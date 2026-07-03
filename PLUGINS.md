# Plugins

Plugins are Python scripts that fetch external data (prices, exchange trades, etc.) and write it into your `sources/` folder. The pipeline rebuilds automatically when new files appear.

## Interface

Plugins are subprocesses spawned by the Arimalo UI when you click **Plugins → Run**. They inherit:

- **Input**: a JSON context on stdin (see [Script Interface](#script-interface) below) plus environment variables.
- **Tools**: the full suite of `arimalo-*` CLI binaries — `arimalo-query` in particular — are on `$PATH` and their absolute paths are provided in `ctx.bin` and `ARIMALO_QUERY_BIN`. Plugins should use these to read ledger data rather than parsing files directly; if the ledger file layout changes, plugins keep working.
- **Permissions**: plugins may write to the vault's `sources/` folder. They **must not** touch `generated/` — that folder is owned by the pipeline and is rebuilt from `sources/` on every change.
- **Output**: files in `sources/`, plus a JSON status summary printed to stdout.

## Location

Plugins live in `plugins/` inside your **data root folder** (the same folder that contains `sources/` and `generated/`). This is the folder you selected when you first launched Arimalo.

```
your-data-root/
  sources/
  generated/
  plugins/          <-- put plugins here
    my-plugin/
      plugin.toml
      sync.py
```

Example plugins are also shipped in the repo under `plugins/` — copy them to your data root to use them.

## Structure

A plugin folder contains a manifest and a script:

```
my-plugin/
  plugin.toml    # manifest (name, config fields, secrets)
  sync.py        # entry point (python3, PEP 723 inline metadata for deps)
  .data/         # auto-created persistent state (git-ignored)
```

### Dependencies (PEP 723 + uv)

Plugins declare their Python dependencies inline in the script, using
[PEP 723 script metadata](https://peps.python.org/pep-0723/). The runner
detects the marker and invokes `uv run --script <file>`, which provisions
an ephemeral, hardlinked venv from a global wheel cache on first run.

Each plugin gets its own venv — there is no shared ecosystem environment,
so plugins can pin different versions and removing a plugin removes its
deps cleanly.

```python
#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = ["yfinance>=0.2", "pandas"]
# ///
"""My plugin docstring."""
import yfinance as yf
...
```

Stdlib-only plugins still declare an empty `dependencies = []` block so
the runner can route through `uv` consistently. If `uv` is not on PATH,
the runner falls back to bare `python3` (which works for stdlib-only
plugins and breaks loudly on plugins that need third-party libraries —
install uv via `brew install uv` or `curl -LsSf https://astral.sh/uv/install.sh | sh`).

`uv` resolves a script's deps lazily on first invocation (~150ms with
the hardlinked wheel cache); subsequent runs are instant. `scripts/run_debug.sh`
does **not** pre-warm — running plugin scripts has side effects (hits APIs,
writes into `sources/`), so we leave dep resolution to the first real click.
The script does check that `uv` is on PATH and prints an install hint if not.

## Manifest

`plugin.toml` declares the plugin metadata, configurable fields, and secret fields:

```toml
[plugin]
name = "My Plugin"
version = "0.1.0"
description = "What this plugin does"
script = "sync.py"
daily = true   # optional; include in the on-startup price backfill (default false)

[config]
# Fields the user can configure in the UI
some_option = { type = "string", default = "hello", description = "An option" }
count = { type = "integer", default = 10, description = "How many things" }
items = { type = "list", default = ["a", "b"], description = "A list of items" }

[secrets]
# Fields stored securely (shown as password inputs, never committed)
api_key = { type = "string", required = true, description = "API key" }
```

### `daily` — automatic startup backfill

Set `daily = true` in `[plugin]` to include the plugin in the **"Update prices
on startup"** batch. When that checkbox (in the Plugins view) is enabled, the
app runs every `daily` plugin shortly after launch, then rebuilds the pipeline
once. The setting is off by default and persists in the app config.

Two rules make this safe to run on every launch:

- **Once per day:** a plugin that already succeeded today (per its
  `.data/last_run.json`) is skipped without running, so repeated launches on the
  same day make no network calls.
- **Incremental & idempotent:** a `daily` plugin **must** fetch only what's
  missing (read its existing `_prices/*.txt`, fetch from the last recorded date
  forward, merge) and treat "nothing new" as a clean no-op — never a wholesale
  re-download or an error. This is what lets a single startup run backfill any
  gap, whether the app was closed for a day or a month.

The price plugins shipped in this repo (`binance-prices`, `coingecko-prices`,
`crypto-yfinance-prices`, `fiat-prices`, `stock-prices`) are all marked `daily`.

## Script Interface

Your script receives a JSON object on **stdin** with everything it needs:

```json
{
  "sources_dir": "/path/to/sources",
  "plugin_dir": "/path/to/plugins/my-plugin",
  "data_dir": "/path/to/plugins/my-plugin/.data",
  "config": { "some_option": "hello", "count": 10 },
  "secrets": { "api_key": "sk-..." },
  "bin": { "arimalo_query": "/path/to/arimalo-query" }
}
```

The same values are also available as environment variables:
- `ARIMALO_SOURCES_DIR`
- `ARIMALO_PLUGIN_DIR`
- `ARIMALO_PLUGIN_DATA_DIR`
- `ARIMALO_QUERY_BIN` (absolute path to `arimalo-query`)
- `PATH` is prepended with the Arimalo bin directory so bare `arimalo-query` also works.

### Querying the ledger

To read transactions, **always go through `arimalo-query`** — never walk `generated/` yourself. The CLI takes an optional base folder (defaults to the whole vault) and walks every `ledger.transactions` beneath it. See `arimalo-query --help` for the full search grammar.

```python
import json, os, subprocess
query_bin = ctx.get("bin", {}).get("arimalo_query") or os.environ["ARIMALO_QUERY_BIN"]
r = subprocess.run(
    [query_bin, "commodity:HNT", "--format", "json"],
    capture_output=True, text=True, check=True,
)
txns = json.loads(r.stdout)["transactions"]
```

### Output

Write files directly into `sources/`:
- **Prices** → `sources/_prices/BTC.txt` (P-directive format: `P 2026-01-15 BTC 42000.00 USD`)
- **Transactions** → `sources/{account-folder}/trades.csv` (processed by existing Rhai transforms)

Never write to `generated/`. The pipeline rebuilds it from `sources/` — any files you put there will be overwritten or stale.

Print a JSON status summary to **stdout**:

```json
{ "files_written": ["_prices/BTC.txt"], "records_fetched": 365, "warnings": [] }
```

### Exit codes

- `0` — success
- `1` — failure (stderr has error message)
- `2` — partial success (some data fetched, some failed)

### Persistent state

Use `data_dir` (the `.data/` folder) to store cursors, last-sync timestamps, or any state between runs. This folder is auto-created and git-ignored.

## Running Plugins

Click **Plugins** in the sidebar, then click **Run** on any plugin. Results (stdout/stderr) are shown in the main content area.

## Included Plugins

### CoinGecko Price Sync

Fetches daily crypto prices from CoinGecko and writes P-directive files.

**Config:**
- `commodities` — CoinGecko coin IDs (e.g. `["bitcoin", "ethereum"]`)
- `quote_currency` — quote currency (e.g. `"usd"`, `"aud"`)
- `lookback_days` — days of history to fetch

**Secrets:**
- `api_key` — optional, for higher rate limits

### Fiat Price Sync

Fetches daily fiat/USD historical exchange rates from Yahoo Finance via
`yfinance` (the same library `external/edwin/global_liquidity.py` uses).
Writes `_prices/AUD.txt`, `_prices/EUR.txt`, etc. with `P` directives.

**Config:**
- `currencies` — comma-separated codes (default `AUD,EUR,GBP`)
- `period` — yfinance period string (default `max`)

**Dependencies:** `yfinance` (declared via PEP 723 — installed automatically by `uv`).

### Hello World (example)

A minimal plugin that demonstrates the interface without calling any external APIs. See `plugins/hello-world/`.

## Writing Your Own Plugin

1. Create a folder in `plugins/` with a `plugin.toml` and a Python script
2. Read config from `json.load(sys.stdin)`
3. Do your work (fetch data, transform, etc.)
4. Write output files to `sources_dir`
5. Print a JSON summary to stdout
6. Exit with code 0 (success), 1 (failure), or 2 (partial)

The runner invokes Python via `uv run --script` when the script has a PEP 723
header and `uv` is on PATH; otherwise it falls back to bare `python3`. Any
language works as long as it reads JSON from stdin and writes files — but
the runner currently only knows how to launch Python scripts.
