const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawn, spawnSync } = require("node:child_process");

const { BeforeAll, AfterAll, Given, Then, setDefaultTimeout } = require("@cucumber/cucumber");
const { Builder, By, Capabilities, until } = require("selenium-webdriver");

setDefaultTimeout(180_000);

const repoRoot = path.resolve(__dirname, "../..");

let appiumProc;
let driver;
let generatedDir;

function onlyForAppium() {
  return process.env.E2E_BACKEND === "appium-mac2";
}

function waitForPort(host, port, timeoutMs) {
  const start = Date.now();
  return new Promise((resolve, reject) => {
    const net = require("node:net");
    const tick = () => {
      const socket = net.createConnection({ host, port });
      socket.once("connect", () => {
        socket.end();
        resolve();
      });
      socket.once("error", () => {
        socket.destroy();
        if (Date.now() - start > timeoutMs) {
          reject(new Error(`Timed out waiting for ${host}:${port}`));
        } else {
          setTimeout(tick, 250);
        }
      });
    };
    tick();
  });
}

function readTauriConfig() {
  // eslint-disable-next-line @typescript-eslint/no-var-requires
  return require(path.join(repoRoot, "src-tauri", "tauri.conf.json"));
}

function resolveMacAppBundlePath() {
  const cfg = readTauriConfig();
  const productName = cfg.package.productName;
  const candidates = [
    path.join(repoRoot, "src-tauri", "target", "debug", "bundle", "macos", `${productName}.app`),
    path.join(repoRoot, "src-tauri", "target", "release", "bundle", "macos", `${productName}.app`),
  ];

  for (const p of candidates) {
    if (fs.existsSync(p)) return p;
  }
  throw new Error(`Unable to locate .app bundle. Build it with: npx tauri build --debug --bundles app`);
}

function ensureMac2DriverInstalled() {
  const res = spawnSync("npx", ["appium", "driver", "list", "--installed"], {
    cwd: repoRoot,
    encoding: "utf8",
    shell: true,
  });
  const out = `${res.stdout ?? ""}${res.stderr ?? ""}`.toLowerCase();
  if (!out.includes("mac2")) {
    throw new Error(
      `Appium mac2 driver is not installed. Run: ./scripts/setup_appium_mac2.sh`,
    );
  }
}

async function startAppium() {
  ensureMac2DriverInstalled();

  appiumProc = spawn("npx", ["appium", "--port", "4723", "--relaxed-security"], {
    cwd: repoRoot,
    stdio: [null, process.stdout, process.stderr],
    shell: true,
  });
  appiumProc.on("exit", (code) => {
    if (code && code !== 0) {
      // eslint-disable-next-line no-console
      console.error(`Appium exited with code ${code}`);
    }
  });

  await waitForPort("127.0.0.1", 4723, 30_000);
}

async function startSessionWithImportedFile(relativePath) {
  const cfg = readTauriConfig();
  const bundleId = cfg.tauri?.bundle?.identifier ?? "com.cog32.arimalocovid";
  const appPath = resolveMacAppBundlePath();

  const importPath = path.resolve(repoRoot, relativePath);
  assert.ok(fs.existsSync(importPath), `Missing import file: ${importPath}`);

  const now = new Date();
  const nowYyyymm = `${now.getFullYear()}${String(now.getMonth() + 1).padStart(2, "0")}`;

  const capabilities = new Capabilities();
  capabilities.set("platformName", "mac");
  capabilities.set("appium:automationName", "mac2");
  capabilities.set("appium:bundleId", bundleId);
  capabilities.set("appium:appPath", appPath);
  capabilities.set("appium:noReset", false);
  generatedDir = fs.mkdtempSync(path.join(os.tmpdir(), "arimalo-covid-appium-generated-"));
  capabilities.set("appium:environment", {
    ARIMALO_E2E_IMPORT_PATHS: importPath,
    ARIMALO_E2E_NOW_YYYYMM: nowYyyymm,
    ARIMALO_GENERATED_DIR: generatedDir,
  });

  driver = await new Builder()
    .withCapabilities(capabilities)
    .usingServer("http://127.0.0.1:4723/")
    .build();
}

async function stopAppium() {
  try {
    if (driver) await driver.quit();
  } finally {
    if (appiumProc) appiumProc.kill();
    if (generatedDir) {
      try {
        fs.rmSync(generatedDir, { recursive: true, force: true });
      } catch {}
    }
  }
}

async function findAnyTextContaining(text, timeoutMs) {
  const needle = text.replaceAll('"', '\\"');
  const xpath = `//*[contains(@name,"${needle}") or contains(@value,"${needle}") or contains(@label,"${needle}")]`;
  await driver.wait(until.elementLocated(By.xpath(xpath)), timeoutMs);
}

BeforeAll(async () => {
  if (!onlyForAppium()) return;
  if (process.platform !== "darwin") {
    // eslint-disable-next-line no-console
    console.log("Skipping Appium mac2 E2E: not macOS.");
    return;
  }

  // Build a debug .app for automation.
  spawnSync("npx", ["tauri", "build", "--debug", "--bundles", "app"], {
    cwd: repoRoot,
    stdio: "inherit",
    shell: true,
    env: { ...process.env, VITE_E2E: "1" },
  });

  await startAppium();
});

AfterAll(async () => {
  if (!onlyForAppium()) return;
  await stopAppium();
});

Given('the macOS app is running with imported file {string}', async (relativePath) => {
  if (!onlyForAppium()) return;
  await startSessionWithImportedFile(relativePath);
});

Then('the UI should show text containing {string}', async (text) => {
  if (!onlyForAppium()) return;
  await findAnyTextContaining(text, 60_000);
});
