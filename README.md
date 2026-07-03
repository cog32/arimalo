
# Overview

- This is a Plain Text Accounting App

- Inspired by an IDE — CSV source files are compiled through transforms and rules, then built into a unified ledger, much like code is compiled and linked into a binary. Warnings (e.g. missing transforms) surface as badges on accounts rather than blocking the entire build.

- Eventually consistent plain text accounting not

- This is an Open Source, [Local First](https://www.inkandswitch.com/essay/local-first/),  Progressive Web App that prioritizes [File Over Application](https://stephango.com/file-over-app)

- In stark contrast to most other accounting apps, your data is your own and you'll never be locked in.

## Data Principles

**The ledger is the single source of truth.** All views, reports, and calculations derive from the transaction data — they never patch or compensate for missing information at display time. When a user action (linking trades, classifying transactions, importing prices) produces new information, it is written back to the source data (transactions, rules, price files). Reports are read-only derivations.

## Format

This is defined in [Ledger Spec](./Ledger.g4)

## Architecture

Arimalo treats your finances like an IDE treats code: raw source files are
**compiled** through transforms and rules into a unified ledger, and every view
and report is derived from that ledger.

```
 sources/  ──▶  transforms + rules  ──▶  generated/  ──▶  reports + UI
 CSV · OFX        _transform.rhai        per-folder ledgers
 manual.txns      _rules.json            + a unified active ledger per account set
```

- **Sources** (`sources/`) are the source of truth — per-account folders holding
  raw exports (CSV, OFX) and hand-written `manual.transactions` (hledger-style). A
  folder's path *is* the account: `richard/crypto/wallet/ethereum/` → `assets:ethereum`.
- **Transforms & rules** are the compiler. Each import folder has a `_transform.rhai`
  (a small [Rhai](https://rhai.rs) script mapping CSV/OFX columns to ledger postings)
  and an optional `_rules.json` (payee/category rules). A missing transform surfaces
  as a warning badge on the account rather than failing the whole build.
- **Incremental build.** A SHA256 content hash plus a directory fingerprint per
  folder lets a rebuild skip everything that didn't change. A file watcher on
  `sources/` recompiles automatically whenever a file changes.
- **Generated** (`generated/`) is the compiled output: per-folder ledgers plus a
  unified active ledger per account set. It is derived and never hand-edited —
  delete it and it rebuilds from `sources/`.
- **Reports** (CGT, income, balances, performance) are generated from the active
  ledger, per account set and financial year.
- **Desktop app.** A [Tauri](https://tauri.app) app — a Rust core with a WebView
  UI. The UI queries the generated ledgers; user actions (import, add/classify
  transactions, link trades) write back to `sources/` and trigger a rebuild, so the
  ledger stays the single source of truth.
- **Sync.** Changes are stored locally in an [Automerge](https://automerge.org) CRDT
  and can be synced across devices through a self-hosted relay (see
  [Multi-Device Sync](#multi-device-sync)).

The CLI mirrors the app: `arimalo-regenerate` runs the same pipeline headlessly,
and `arimalo-query` reads the generated ledgers (see [Development](#development)).

## Plugins

Plugins fetch external data — daily prices, on-chain / exchange transaction
history — and write it into `sources/` (price files, per-wallet CSVs), where it
flows through the normal pipeline. They keep Arimalo current without baking any
external API into the core, and honour the [data principle](#data-principles):
they only ever write to `sources/`.

**Where they live (and how they deploy).** The canonical plugins are
version-controlled in this repo under `plugins/`. They are *installed into the
active vault* by `scripts/run_debug.sh`, which `rsync`s `plugins/` →
`<vault>/plugins/` with `--delete` (the vault mirrors the repo) while
**excluding `.data/`** so each vault's config and secrets are preserved. Author
plugins **in the repo, not the vault** — a deploy overwrites anything vault-only.

**Structure.** Each plugin is a folder containing:

- `plugin.toml` — the manifest: `name`, `version`, `script`, a `daily` flag
  (whether it runs in the startup price backfill), a `[config]` schema (typed
  fields with defaults), and an optional `[secrets]` schema (e.g. API keys).
- `sync.py` — a [PEP 723](https://peps.python.org/pep-0723/) script (dependencies
  declared inline, resolved lazily by [`uv`](https://docs.astral.sh/uv/)). It
  reads a JSON context from **stdin** — `{config, secrets, sources_dir,
  data_dir}` — writes/merges files under `sources/`, and prints a JSON result
  (`{files_written, warnings}`) to **stdout**.
- `.data/` — per-vault runtime state: `config.json` (the live config the runner
  passes to the script), `secrets.json`, and `last_run.json`. **Not** in the
  repo; preserved across deploys.

**Running.** The "Update prices on startup" option runs every `daily = true`
plugin once per day (`run_daily_plugins`), and the **Plugins** view has a **Run**
button per plugin. Price plugins are incremental — they merge new dates into the
existing `sources/_prices/{SYMBOL}.txt` P-directive files rather than rewriting
history.

**Price sources.** Crypto spot prices come from a single **`crypto-spot-prices`**
plugin that tries **Binance → CoinGecko → yfinance** per coin, taking the first
*current* price (it rejects data frozen at a delisting and falls through);
wrapped/staked tokens (WETH, WBTC, stETH, mSOL) are priced from their underlying.
A free CoinGecko demo key (`coingecko_api_key` secret) lifts the fallback's rate
limit. Separately, **`binance-prices`** backfills *transaction-time* prices for
accurate CGT cost basis (a distinct job). Equities/ETFs use **`stock-prices`**;
fiat FX uses **`fiat-prices`**.

## UI

- Click `Import` to bring a CSV or OFX file into the selected account (Arimalo suggests a column mapping).
- The left sidebar lists accounts and their running balances (per commodity, preferring `USD` for display).
- The main table shows transactions affecting the selected account, with `Payment` / `Deposit` split from the selected account’s posting amount.

## Demos

Short, muted screencasts of the three main flows. Regenerate them anytime with
`npm run record:demos` (see [docs/demos/RECORDING.md](docs/demos/RECORDING.md)).

### Getting started — choose a data folder and add an account

<video controls muted loop playsinline width="720" poster="docs/demos/getting-started-poster.png">
  <source src="docs/demos/getting-started.mp4" type="video/mp4" />
  <source src="docs/demos/getting-started.webm" type="video/webm" />
</video>

### Import a CSV

<video controls muted loop playsinline width="720" poster="docs/demos/import-csv-poster.png">
  <source src="docs/demos/import-csv.mp4" type="video/mp4" />
  <source src="docs/demos/import-csv.webm" type="video/webm" />
</video>

### Add a manual transaction

<video controls muted loop playsinline width="720" poster="docs/demos/manual-txn-poster.png">
  <source src="docs/demos/manual-txn.mp4" type="video/mp4" />
  <source src="docs/demos/manual-txn.webm" type="video/webm" />
</video>

## Install

Download the latest release from the [Releases](../../releases) tab.

### macOS

1. Download the `.dmg` file
2. Open the disk image and drag **Arimalo COVID** to Applications
3. On first launch, macOS may block the app. Go to **System Settings > Privacy & Security** and click **Open Anyway**

Alternatively, download the `.app.zip`, unzip it, and move the `.app` to Applications.

### Windows

Download **either** installer:

- `.msi` — standard Windows Installer (add/remove programs support)
- `.exe` — NSIS installer

Run the installer and follow the prompts.

### Linux (Debian / Ubuntu)

```sh
sudo dpkg -i arimalo-covid_*_amd64.deb
```

### Linux (Arch)

Use the `.AppImage`:

```sh
chmod +x arimalo-covid_*_amd64.AppImage
./arimalo-covid_*_amd64.AppImage
```

Or convert the `.deb` with [`debtap`](https://aur.archlinux.org/packages/debtap):

```sh
debtap arimalo-covid_*_amd64.deb
sudo pacman -U arimalo-covid-*.pkg.tar.zst
```

### Linux (other)

The `.AppImage` runs on any distribution with FUSE support:

```sh
chmod +x arimalo-covid_*_amd64.AppImage
./arimalo-covid_*_amd64.AppImage
```

## Multi-Device Sync

Arimalo uses a self-hosted relay server (`arimalo-relay`) to sync between devices.

The app is **local-first** — all changes are saved immediately to your local Automerge document and content store, regardless of whether the relay server is running. The relay is purely optional and only used when you click **Sync Now**. If the relay is unavailable, you'll see an error but no data is lost. When the relay comes back online, the next sync performs a full reconciliation and all offline changes from every device are merged automatically.

### Run the relay server

The relay binary is included in each release. Download it for your platform from the [Releases](../../releases) tab, then run:

```sh
arimalo-relay
```

By default it listens on `0.0.0.0:8384` and stores data in `/tmp/arimalo-relay`.

**Options:**

```
--bind <ADDR>      Bind address (default: 0.0.0.0:8384)
--data-dir <PATH>  Data directory (default: /tmp/arimalo-relay)
```

Example with a persistent data directory:

```sh
arimalo-relay --bind 0.0.0.0:8384 --data-dir /var/lib/arimalo-relay
```

### Pair devices

1. On one device, click **Pair Device** in the sidebar and choose **Create** — note the 6-digit code.
2. On the other device, click **Pair Device**, choose **Join**, and enter the code.
3. Click **Sync Now** on either device to exchange data.

## Development

### Run the app

- Web (Vite): `npm run dev`
- Desktop (Tauri): `npm run tauri:dev`

### Data file locations

All app data lives under the platform app-data directory:

| Platform | Path |
|----------|------|
| macOS    | `~/Library/Application Support/com.cog32.arimalocovid/sources/` |
| Linux    | `~/.local/share/com.cog32.arimalocovid/sources/` |
| Windows  | `%APPDATA%\com.cog32.arimalocovid\sources\` |

Key files:

- `arimalo-metadata.automerge` — CRDT sync metadata (delete to reset sync state)
- `relay-config.json` — paired relay URL and group ID (delete to unpair)

### Run the relay server locally

From the `src-tauri` directory:

```sh
cargo run --bin arimalo-relay
```

With options:

```sh
cargo run --bin arimalo-relay -- --bind 127.0.0.1:8384 --data-dir /tmp/arimalo-relay
```

`cargo run` builds in debug mode by default. Add `--release` for an optimised build.

### Tests

- Parser BDD (Rust/Cucumber): `npm run test:bdd`
- UI E2E (Tauri WebDriver + Cucumber): a `selenium-webdriver` DOM suite. `npm run e2e`
  auto-selects the WebDriver server for your OS and skips (with a hint) if it isn't installed.
  - **Linux/Windows** — `tauri-driver`: `cargo install tauri-driver --locked`
  - **macOS** — `tauri-wd` (`tauri-driver` has no macOS backend):
    `cargo install tauri-webdriver-automation`, then grant the terminal Accessibility +
    Automation permission. Run it explicitly with `npm run e2e:wd`.
  - Run: `npm run e2e` (or `scripts/run_e2e_tests.sh`). Details: `features/ui/README.md`.

- macOS Appium mac2 — a separate native-AX smoke (`@appium` scenarios only, not the DOM suite):
  - Setup once: `./scripts/setup_appium_mac2.sh`
  - Run: `npm run e2e:mac`

- Interactive debugging (MCP + Claude Code):
  - An [mcp-tauri-automation](https://github.com/danielraffel/mcp-tauri-automation) server is configured in `.mcp.json`
  - Install `tauri-wd` (macOS WebDriver): `cargo install tauri-webdriver-automation`
  - Start: `tauri-wd --port 4444`
  - Build the app: `npx tauri build --debug --features webdriver`
  - Claude Code can then launch the app, take screenshots, click elements, and execute JS in the WebView
  - **Note:** Use `npx tauri build --debug` (not bare `cargo build`) — the latter does not embed the frontend, causing a blank screen
