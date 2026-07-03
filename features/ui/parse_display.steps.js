const assert = require("node:assert/strict");
const os = require("node:os");
const path = require("node:path");
const fs = require("node:fs");
const net = require("node:net");
const { spawn, spawnSync } = require("node:child_process");

const { Before, BeforeAll, AfterAll, Given, When, Then, setDefaultTimeout } = require("@cucumber/cucumber");
const { Builder, By, Capabilities, until } = require("selenium-webdriver");

setDefaultTimeout(120_000);

const repoRoot = path.resolve(__dirname, "../..");
const tauriDriverPath = path.resolve(os.homedir(), ".cargo", "bin", "tauri-driver");
const tauriWdPath = path.resolve(os.homedir(), ".cargo", "bin", "tauri-wd");
// macOS/tauri-wd builds go into this isolated target dir (matching
// scripts/run_debug_mcp.sh) so the `webdriver` feature's extra crates don't
// churn the main src-tauri/target shared with `cargo test` / `tauri dev`.
const mcpTargetDir = path.join(os.homedir(), ".cache", "arimalo-target-mcp");

let driverServer; // spawned tauri-driver (Linux/Windows) or tauri-wd (macOS)
let driver;
let generatedDir;

// WebDriver backend that drives the suite:
//   - "tauri-driver": Tauri's WebDriver intermediary (Linux/Windows), DOM-level.
//   - "tauri-wd":     WKWebView WebDriver for macOS (DOM-level). Default on macOS
//                     because tauri-driver has no macOS backend.
//   - "appium-mac2":  native AX automation, owned by appium_macos.steps.js
//                     (@appium scenarios only) — this file no-ops for it.
// Override with the E2E_BACKEND env var; otherwise it is chosen by platform.
function selectedBackend() {
  const explicit = process.env.E2E_BACKEND;
  if (explicit === "appium-mac2" || explicit === "tauri-wd" || explicit === "tauri-driver") {
    return explicit;
  }
  return process.platform === "darwin" ? "tauri-wd" : "tauri-driver";
}

function mkTempDir(prefix) {
  return fs.mkdtempSync(path.join(os.tmpdir(), `${prefix}-`));
}

function resolveAppBinaryPath(backend) {
  // tauri-wd loads the binary built (with the `webdriver` feature) into the
  // isolated MCP target dir; tauri-driver uses the regular debug target dir.
  const targetDir =
    backend === "tauri-wd"
      ? path.join(mcpTargetDir, "debug")
      : path.join(repoRoot, "src-tauri", "target", "debug");

  const platformExt = process.platform === "win32" ? ".exe" : "";
  for (const name of ["arimalo-covid", "arimalo_covid", "tauri-app", "app"]) {
    const candidate = path.join(targetDir, `${name}${platformExt}`);
    if (fs.existsSync(candidate)) return candidate;
  }

  // macOS may output an .app even with --no-bundle, depending on Tauri version/config.
  const bundleDir = path.join(targetDir, "bundle", "macos");
  if (fs.existsSync(bundleDir)) {
    for (const app of fs.readdirSync(bundleDir).filter((f) => f.endsWith(".app"))) {
      const binName = path.basename(app, ".app");
      const candidate = path.join(bundleDir, app, "Contents", "MacOS", binName);
      if (fs.existsSync(candidate)) return candidate;
    }
  }

  const hint =
    backend === "tauri-wd"
      ? 'Build with: npm run build && CARGO_TARGET_DIR=~/.cache/arimalo-target-mcp cargo build --manifest-path src-tauri/Cargo.toml --bin arimalo-covid --features "webdriver tauri/custom-protocol"'
      : "Run: npx tauri build --debug --no-bundle";
  throw new Error(`Unable to locate built Tauri binary under ${targetDir}. ${hint}`);
}

function waitForPort(host, port, timeoutMs) {
  const start = Date.now();
  return new Promise((resolve, reject) => {
    const tick = () => {
      const socket = net.createConnection({ host, port });
      socket.once("connect", () => {
        socket.end();
        resolve();
      });
      socket.once("error", () => {
        socket.destroy();
        if (Date.now() - start > timeoutMs) reject(new Error(`Timed out waiting for ${host}:${port}`));
        else setTimeout(tick, 200);
      });
    };
    tick();
  });
}

function buildApp(backend) {
  if (backend === "tauri-wd") {
    // 1) Frontend with the E2E DOM hooks (#e2e-parse-path, [data-testid=app-ready]),
    //    gated behind VITE_E2E. tauri/custom-protocol embeds this dist/ at link time.
    const fe = spawnSync("npm", ["run", "build"], {
      cwd: repoRoot,
      stdio: "inherit",
      shell: true,
      env: { ...process.env, VITE_E2E: "1" },
    });
    if (fe.status !== 0) throw new Error("Frontend build failed (npm run build).");

    // 2) App binary with tauri-plugin-webdriver-automation (the `webdriver`
    //    feature) so tauri-wd can attach, built into the isolated target dir.
    const app = spawnSync(
      "cargo",
      [
        "build",
        "--manifest-path",
        "src-tauri/Cargo.toml",
        "--bin",
        "arimalo-covid",
        // Comma-separated (not space) so it survives as ONE shell word under
        // spawnSync's shell:true, which would otherwise split it into a stray arg.
        "--features",
        "webdriver,tauri/custom-protocol",
      ],
      {
        cwd: repoRoot,
        stdio: "inherit",
        shell: true,
        env: { ...process.env, CARGO_TARGET_DIR: mcpTargetDir },
      },
    );
    if (app.status !== 0) throw new Error("App build failed (cargo build --features webdriver).");
    return;
  }

  spawnSync("npx", ["tauri", "build", "--debug", "--bundles", "none"], {
    cwd: repoRoot,
    stdio: "inherit",
    shell: true,
    env: { ...process.env, VITE_E2E: "1" },
  });
}

function startDriverServer(backend) {
  if (backend === "tauri-wd") {
    if (!fs.existsSync(tauriWdPath)) {
      throw new Error(
        `tauri-wd is not installed at ${tauriWdPath}. Install with: cargo install tauri-webdriver-automation`,
      );
    }
    // tauri-wd owns port 4444 exclusively; drop any stale server (e.g. one left
    // running for MCP debugging) so this suite drives the binary we just built.
    spawnSync("pkill", ["-f", "tauri-wd"]);
    // tauri-wd ignores capabilities' `tauri:options.env`; the launched app
    // inherits THIS process's env, so the generated-dir override goes here.
    driverServer = spawn(tauriWdPath, ["--port", "4444"], {
      stdio: [null, process.stdout, process.stderr],
      env: { ...process.env, ARIMALO_GENERATED_DIR: generatedDir },
    });
  } else {
    if (!fs.existsSync(tauriDriverPath)) {
      throw new Error(
        `tauri-driver is not installed at ${tauriDriverPath}. Install with: cargo install tauri-driver --locked`,
      );
    }
    driverServer = spawn(tauriDriverPath, [], {
      stdio: [null, process.stdout, process.stderr],
      env: { ...process.env, ARIMALO_GENERATED_DIR: generatedDir },
    });
  }
}

// tauri-wd surfaces the WKWebView window handle ~1-2s after the session starts;
// it must be selected before the DOM is queryable. (tauri-driver auto-focuses.)
async function selectWebviewWindow(timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  let handles = [];
  while (Date.now() < deadline) {
    try {
      handles = await driver.getAllWindowHandles();
    } catch {
      handles = [];
    }
    if (handles.length) break;
    await driver.sleep(200);
  }
  if (!handles.length) throw new Error("tauri-wd: app window did not appear");
  await driver.switchTo().window(handles[0]);
}

async function startWebdriverSession() {
  const backend = selectedBackend();

  buildApp(backend);

  generatedDir = mkTempDir("arimalo-covid-e2e-generated");
  startDriverServer(backend);
  await waitForPort("127.0.0.1", 4444, 30_000);

  const capabilities = new Capabilities();
  capabilities.set("tauri:options", { application: resolveAppBinaryPath(backend) });
  capabilities.setBrowserName("wry");

  driver = await new Builder()
    .withCapabilities(capabilities)
    .usingServer("http://127.0.0.1:4444/")
    .build();

  if (backend === "tauri-wd") await selectWebviewWindow(20_000);

  await driver.wait(until.elementLocated(By.css('[data-testid="app-ready"]')), 30_000);
}

async function stopWebdriverSession() {
  try {
    if (driver) await driver.quit();
  } finally {
    if (driverServer) driverServer.kill();
    if (generatedDir) {
      try {
        fs.rmSync(generatedDir, { recursive: true, force: true });
      } catch {}
    }
  }
}

BeforeAll(async () => {
  if (selectedBackend() === "appium-mac2") return; // appium_macos.steps.js owns that path
  await startWebdriverSession();
});

AfterAll(async () => {
  if (selectedBackend() === "appium-mac2") return;
  await stopWebdriverSession();
});

// Bridge the driver onto the cucumber World so step files can use `this.driver`
// (the convention in categories/edit_rule/hide_transaction/…) alongside the
// module-level `driver` used by this file's own steps.
Before(function () {
  if (selectedBackend() !== "appium-mac2") this.driver = driver;
});

Given("the app is running", async () => {
  const el = await driver.findElement(By.css('[data-testid="app-ready"]'));
  assert.equal(await el.getAttribute("data-testid"), "app-ready");
});

When('I parse the transactions file {string}', async (relativePath) => {
  const fixturePath = path.resolve(repoRoot, relativePath);
  assert.ok(fs.existsSync(fixturePath), `fixture file missing: ${fixturePath}`);

  const input = await driver.findElement(By.css("#e2e-parse-path"));
  await input.clear();
  await input.sendKeys(fixturePath);

  const run = await driver.findElement(By.css("#e2e-parse-run"));
  await run.click();

  await driver.wait(
    until.elementTextContains(
      driver.findElement(By.css('[data-testid="parse-status"]')),
      "Parsed",
    ),
    30_000,
  );
});

Then('the sidebar should include the account {string}', async (account) => {
  const selector = `[data-testid="account-item"][data-account="${account}"]`;
  await driver.wait(until.elementLocated(By.css(selector)), 10_000);
});

Then('the sidebar should include the account group {string}', async (group) => {
  const selector = `[data-testid="account-group"][data-group="${group}"]`;
  await driver.wait(until.elementLocated(By.css(selector)), 10_000);
});

Then('the sidebar should not include the account group {string}', async (group) => {
  const selector = `[data-testid="account-group"][data-group="${group}"]`;
  const elements = await driver.findElements(By.css(selector));
  assert.equal(elements.length, 0, `Expected account group "${group}" to not be in the sidebar`);
});

Then('the account group {string} should include the account {string}', async (group, account) => {
  const selector = `[data-testid="account-group"][data-group="${group}"] [data-testid="account-item"][data-account="${account}"]`;
  await driver.wait(until.elementLocated(By.css(selector)), 10_000);
});

Then('the account {string} should show an amount of {string}', async (account, amount) => {
  const selector = `[data-testid="account-item"][data-account="${account}"]`;
  const item = await driver.findElement(By.css(selector));
  const amt = await item.findElement(By.css(".account__amt"));
  assert.equal(await amt.getText(), amount);
});

When('I select the account {string}', async (account) => {
  const selector = `[data-testid="account-item"][data-account="${account}"]`;
  const item = await driver.findElement(By.css(selector));
  await item.click();

  await driver.wait(
    until.elementTextContains(
      driver.findElement(By.css('[data-testid="selected-account"]')),
      account,
    ),
    10_000,
  );
});

Then('I should see a transaction row for payee {string}', async (payee) => {
  const row = await driver.findElement(By.css('[data-testid="txn-row"]'));
  const cell = await row.findElement(By.css('[data-testid="txn-payee"]'));
  assert.equal(await cell.getText(), payee);
});

Then('I should see a notes value of {string}', async (notes) => {
  const row = await driver.findElement(By.css('[data-testid="txn-row"]'));
  const cell = await row.findElement(By.css('[data-testid="txn-notes"]'));
  assert.equal(await cell.getText(), notes);
});

Then('that transaction row should show a deposit of {string}', async (amount) => {
  const row = await driver.findElement(By.css('[data-testid="txn-row"]'));
  const deposit = await row.findElement(By.css('[data-testid="txn-deposit"]'));
  assert.equal(await deposit.getText(), amount);
});

When('I add a manual transaction with payee {string} and notes {string}', async (payee, notes) => {
  const addNew = await driver.findElement(By.css("#addNew"));
  await addNew.click();

  await driver.wait(until.elementLocated(By.css("#manualPayee")), 10_000);
  const payeeInput = await driver.findElement(By.css("#manualPayee"));
  const notesInput = await driver.findElement(By.css("#manualNarration"));

  await payeeInput.clear();
  await payeeInput.sendKeys(payee);
  await notesInput.clear();
  await notesInput.sendKeys(notes);

  // Value mode (default): an amount into the selected account...
  const amount = await driver.findElement(By.css("#manualAmount"));
  await amount.clear();
  await amount.sendKeys("3.50");
  // ...and one "other account" row whose blank amount auto-balances it.
  const contra = await driver.findElement(By.css("#manualContraAccount-0"));
  await contra.clear();
  await contra.sendKeys("expenses:coffee");

  const save = await driver.findElement(By.css("#manualSave"));
  await save.click();

  await driver.wait(until.elementTextContains(driver.findElement(By.css('[data-testid="parse-status"]')), "added manual"), 30_000);
});

When('I click the "Add New" button', async () => {
  const addNew = await driver.findElement(By.css("#addNew"));
  await addNew.click();
  await driver.wait(until.elementLocated(By.css('[data-testid="manual-txn-modal"]')), 10_000);
});

Then('the account field should show {string}', async (expected) => {
  const acct = await driver.findElement(By.css(".manualAccount"));
  const value = await acct.getText();
  assert.ok(value.includes(expected), `Expected account field to show "${expected}" but got "${value}"`);
});

// Add Account feature steps
When('I click the "Add Account" button', async () => {
  const addAccountBtn = await driver.findElement(By.css('[data-testid="add-account-btn"]'));
  await addAccountBtn.click();
  await driver.wait(until.elementLocated(By.css('[data-testid="add-account-modal"]')), 10_000);
});

When('I enter account name {string}', async (accountName) => {
  const input = await driver.findElement(By.css('#newAccountName'));
  await input.clear();
  await input.sendKeys(accountName);
});

When('I submit the new account form', async () => {
  const submitBtn = await driver.findElement(By.css('#addAccountSubmit'));
  await submitBtn.click();
  // Wait for modal to close
  await driver.wait(async () => {
    const modals = await driver.findElements(By.css('[data-testid="add-account-modal"]'));
    return modals.length === 0;
  }, 10_000);
});
