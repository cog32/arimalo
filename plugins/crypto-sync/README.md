# Crypto Wallet Sync Plugin

Pulls transaction history for wallets and exchange accounts and writes one CSV per wallet under `sources/`.

## How it works

1. Reads `plugin_dir/wallets.json` (configurable via `wallets_file`).
2. For each source, picks a provider:
   - `provider: "zerion"` → Zerion `/v1/wallets/{address}/transactions/`. Solana wallets get supplemental Helius RPC enrichment; Ethereum-family wallets get supplemental Etherscan metadata.
   - Ethereum without `provider` → Etherscan v2 (`txlist`, `txlistinternal`, `tokentx`).
   - Solana without `provider` → Solana RPC (`getSignaturesForAddress` + `getTransaction`).
   - Cosmos → Mintscan.
   - Binance exchange accounts → Binance accounting endpoint.
3. Writes per-wallet CSVs at `sources/<output_subpath>/<chain>/<address>/<chain>_<address>_transactions.csv`. Incremental — only new transactions are appended.
4. Sync state JSON files (`<chain>_<address>_state.json`) live in `.data/state/`, never in `sources/`.

## Install

```bash
pip install requests pydantic
```

Arimalo does not yet manage per-plugin Python deps; install into whichever Python the host invokes.

## Configuration

- `wallets_file` — path (relative to plugin dir) to the wallets JSON. Default: `wallets.json`.
- `output_subpath` — subfolder under `sources/`. Default: `richard/crypto/wallet`.
- `binance_max_rows` — cap rows per Binance source (0 = unlimited).

## Secrets

Set these via the Arimalo UI (stored in `.data/secrets.json`):

| Secret | Required | Purpose |
|---|---|---|
| `zerion_api_key` | for Zerion-provider wallets | Zerion API |
| `etherscan_api_key` | recommended for Ethereum | Etherscan v2 |
| `helius_api_key` | recommended for Solana | Helius RPC (Zerion enrichment) |
| `solana_rpc_url` | optional | Override Solana RPC if no Helius key |
| `mintscan_api_key` | for Cosmos wallets | Mintscan |
| `binance_api_key` / `binance_api_secret` | for Binance | Read-only Binance API |

## Raw API response cache

Optional disk cache of every API page fetched during sync, so subsequent runs can replay from local files without hitting Zerion / Etherscan / Helius. Useful for offline re-runs, faster transform iteration, and inspecting raw upstream data.

The Arimalo-driven plugin invocation always runs in cache-off mode (live API). Cache modes are accessed via the bootstrap script or `sync_crypto.cli` directly.

### Build / replay

```bash
# Build (slow — full history; 12-wallet vault ≈ 2-3 hr; consumes API credits)
uv run scripts/build_cache.py

# Custom cache location:
uv run scripts/build_cache.py --cache-dir /path/to/cache

# Or via the underlying CLI (flat output dir; mutually exclusive flags):
uv run -m sync_crypto.cli wallets.json -o output --update-cache
uv run -m sync_crypto.cli wallets.json -o output --from-cache
```

### Layout

Default cache dir: `<vault>/plugins/crypto-sync/.data/cache/` (alongside `secrets.json` + `state/`; covered by `.gitignore`).

```
<cache>/zerion/<address>/page-NNNN.json + index.json
<cache>/etherscan/<chain>/<address>/<txlist|txlistinternal|tokentx>/page-NNNN.json + index.json
<cache>/etherscan/<chain>/proxy/<action>/<txhash|"latest">.json
<cache>/solana_rpc/signatures/<address>/page-NNNN.json + index.json
<cache>/solana_rpc/transactions/<signature>.json
```

EVM addresses are normalized to lowercase in paths; Solana base58 is preserved verbatim. Each `index.json` carries a `cache_schema_version` (currently `1`) for future invalidation.

### Coverage

Cached: Zerion paginated, Etherscan paginated (standalone EVM + native-ETH supplement), Etherscan proxy (per-tx receipts used by Zerion EVM enrichment), Solana RPC for standalone Solana sync (signatures + per-tx).

Not cached: Mintscan (Cosmos — `build_cache.py` skips Cosmos wallets), and Solana RPC `jsonParsed` calls used by Zerion's Solana enrichment (different encoding from standalone sync; would need a separate cache namespace).

### Modes

- **OFF (default)**: live API only, no cache I/O. Existing call signatures preserved.
- **WRITE (`--update-cache`)**: live API, tee each fetched page through the cache. Forces full history (`state={}` for Zerion, `start_block=0` for Etherscan, `until=None` for Solana). New pages append at the next free index slot — historical pages are preserved across re-builds.
- **READ (`--from-cache`)**: cache only, no API calls. Wallets without cached data are skipped with a warning (per-wallet, not fatal).

### Cost reference

Initial 12-wallet build (May 2026): 2 h 19 min → 466 MB / ~3.7 K files. Time dominated by per-tx Etherscan proxy enrichment for the active ETH wallets. Subsequent `--from-cache` replays are bound by disk I/O only.

To force a complete rebuild, `rm -rf <cache>/` and re-run `build_cache.py`.

## `wallets.json` schema

```json
{
  "ethereum": [
    {"friendly_name": "main", "address": "0xABC...", "provider": "zerion"}
  ],
  "solana": [
    {"friendly_name": "sol_main", "address": "ABC..."}
  ],
  "cosmos": [
    {"friendly_name": "atom1", "address": "cosmos1..."}
  ],
  "binance": [
    {"friendly_name": "main_binance", "symbols": ["BTCUSDT", "ETHUSDT"]}
  ]
}
```

Optional per-wallet fields: `network` (e.g. `arbitrum`), `provider` (`zerion` switches to Zerion).

## Migrating from edwin-computer-use's `sync_crypto`

State files written by the legacy CLI sat alongside CSVs in a flat `output/` dir. To preserve incremental sync:

```bash
mkdir -p .data/state
cp ~/workspace/edwin-computer-use/output/*_state.json .data/state/
```

CSVs already at `sources/<output_subpath>/<chain>/<addr>/...` need no migration.
