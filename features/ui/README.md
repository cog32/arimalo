# UI / E2E BDD (Cucumber)

UI/E2E `.feature` files executed with `cucumber-js`.

These are real WebDriver tests: a `selenium-webdriver` client (`By.css(...)`
DOM queries) drives the app's WebView. The only platform difference is which
WebDriver **server** serves `127.0.0.1:4444` — the feature files and steps are
identical everywhere.

## Requirements

- Node dependency `selenium-webdriver` installed (via `npm install`)

### Linux / Windows — `tauri-driver`

- Install: `cargo install tauri-driver --locked`
- Wraps the platform WebView driver (WebKitWebDriver / Edge WebDriver).

### macOS — `tauri-wd`

`tauri-driver` has no macOS backend (Apple ships no WKWebView WebDriver), so on
macOS the suite uses **`tauri-wd`** ([tauri-webdriver-automation]) — a W3C
WebDriver server that drives the WKWebView DOM directly. It's the same server
the `tauri-automation` MCP and `scripts/record_demos.mjs` already use.

- Install: `cargo install tauri-webdriver-automation` (provides `tauri-wd`)
- Grant **Accessibility + Automation** permission to your terminal
  (System Settings → Privacy & Security) — the WebDriver session can't drive the
  app window without it.
- Stop any running `arimalo-covid` (e.g. `tauri dev`) first. `tauri-wd` owns
  port 4444; the suite `pkill`s stale `tauri-wd` servers to take the port.

[tauri-webdriver-automation]: https://github.com/danielraffel/tauri-webdriver

## Run

- `npm run e2e` — runs the suite, auto-selecting the backend for the current OS
  (`tauri-driver` on Linux/Windows, `tauri-wd` on macOS). Skips with a hint if
  the backend isn't installed.
- `npm run test:bdd:ui` — bare `cucumber-js`; same auto-selected backend.
- `npm run e2e:wd` — macOS DOM suite, explicitly forcing the `tauri-wd` backend
  (the non-skipping counterpart to `npm run e2e`).
- Force a backend with `E2E_BACKEND=tauri-wd|tauri-driver|appium-mac2`.

### macOS — Appium mac2 (native-AX smoke, separate)

`npm run e2e:mac` runs the `@appium` scenarios via Appium's `mac2` driver. That
path automates the **native** macOS app through Accessibility (XCUITest), not
the DOM, so it only covers a coarse smoke check — it is **not** how the DOM
feature files above run. Requires Xcode + `./scripts/setup_appium_mac2.sh`.

## How it works

The driver lifecycle lives in `parse_display.steps.js` (`BeforeAll` / `AfterAll`),
keyed on `selectedBackend()`:

1. **Build.** `tauri-driver` → `tauri build --debug --bundles none`.
   `tauri-wd` → `npm run build` then
   `cargo build --bin arimalo-covid --features "webdriver tauri/custom-protocol"`
   into the isolated `~/.cache/arimalo-target-mcp` target dir. The `webdriver`
   feature embeds `tauri-plugin-webdriver-automation` so `tauri-wd` can attach;
   the isolated target dir keeps that feature's extra crates from churning the
   main `src-tauri/target`. Both builds set `VITE_E2E=1` so the frontend exposes
   the E2E hooks (`#e2e-parse-path`, `[data-testid="app-ready"]`).
2. **Server.** Spawn `tauri-driver` or `tauri-wd --port 4444`. `ARIMALO_GENERATED_DIR`
   (a throwaway temp dir) is set on the **server process** — `tauri-wd` ignores
   the capabilities' `tauri:options.env`, so the launched app inherits it from
   the server instead.
3. **Session.** Identical capabilities everywhere: `tauri:options.application` =
   the built binary, `browserName: "wry"`, server `http://127.0.0.1:4444/`. On
   `tauri-wd` the WKWebView window handle appears ~1–2 s later, so the session
   selects it (`getAllWindowHandles` → `switchTo().window`) before querying.
4. **World bridge.** A `Before` hook publishes `this.driver = driver` so every
   step file can use `this.driver` (the convention in `categories` / `edit_rule` /
   `hide_transaction` / …) while this file's own steps use the module-level
   `driver`.

## CI gating: undefined steps

`scripts/run_feature_tests.sh` (run by the pre-commit hook and CI) does a
`cucumber-js --dry-run` over this directory and fails if any non-`@wip`
scenario has an undefined step. This is what stops the suite from silently
growing scenarios that never execute.

Scenarios whose step definitions are not yet wired up must be tagged
`@wip` at the feature level. Removing the tag is the trigger for actually
implementing the step file. `@appium` scenarios are mac-only and excluded
from the gate the same way.
