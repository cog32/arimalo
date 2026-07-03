#!/usr/bin/env python3
"""Crypto Wallet Sync plugin.

Reads context from stdin, then iterates wallet/exchange sources in
wallets.json calling into the vendored sync_crypto package. Per-wallet
CSVs land at:

    sources_dir / output_subpath / <chain> / <address> / <chain>_<addr>_transactions.csv

State JSON files live in the plugin's .data/state/ — never in sources/.
"""

import json
import logging
import os
import sys
from pathlib import Path

logging.basicConfig(level=logging.INFO, format="%(levelname)s: %(message)s", stream=sys.stderr)
logger = logging.getLogger("crypto-sync")


def _set_env_from_secrets(secrets: dict) -> None:
    mapping = {
        "zerion_api_key": "ZERION_API_KEY",
        "etherscan_api_key": "ETHERSCAN_API_KEY",
        "helius_api_key": "HELIUS_API_KEY",
        "solana_rpc_url": "SOLANA_RPC_URL",
        "mintscan_api_key": "MINTSCAN_API_KEY",
        "binance_api_key": "BINANCE_API_KEY",
        "binance_api_secret": "BINANCE_API_SECRET",
    }
    for secret_key, env_name in mapping.items():
        value = (secrets.get(secret_key) or "").strip()
        if value:
            os.environ[env_name] = value


def _wallet_output_dir(base: Path, source) -> Path:
    """Per-wallet nested dir to match accountsv2 layout: <base>/<chain>/<address>."""
    from sync_crypto.models import BinanceConfig, WalletConfig

    if isinstance(source, WalletConfig):
        return base / source.blockchain.value / source.address
    if isinstance(source, BinanceConfig):
        return base / "binance" / source.friendly_name
    return base


def _solana_rpc_url() -> str:
    from sync_crypto.clients import SolanaRpcClient

    if (os.environ.get("HELIUS_API_KEY") or "").strip():
        return SolanaRpcClient.resolve_rpc_url()
    return (os.environ.get("SOLANA_RPC_URL") or "").strip() or None


def main() -> None:
    ctx = json.load(sys.stdin)
    plugin_dir = Path(ctx["plugin_dir"])
    sources_dir = Path(ctx["sources_dir"])
    data_dir = Path(ctx["data_dir"])
    config = ctx.get("config") or {}
    secrets = ctx.get("secrets") or {}

    sys.path.insert(0, str(plugin_dir))

    _set_env_from_secrets(secrets)

    from sync_crypto.models import BinanceConfig, WalletConfig
    from sync_crypto.sync import load_config, sync_source

    wallets_file = plugin_dir / config.get("wallets_file", "wallets.json")
    if not wallets_file.exists():
        result = {
            "files_written": [],
            "records_fetched": 0,
            "warnings": [f"wallets file not found: {wallets_file}"],
        }
        print(json.dumps(result))
        sys.exit(1)

    sources = load_config(wallets_file)
    base_output = sources_dir / config.get("output_subpath", "richard/crypto/wallet")
    state_dir = data_dir / "state"
    state_dir.mkdir(parents=True, exist_ok=True)

    binance_max_rows = config.get("binance_max_rows") or None
    if binance_max_rows == 0:
        binance_max_rows = None

    files_written: list[str] = []
    warnings: list[str] = []

    for source in sources:
        wallet_dir = _wallet_output_dir(base_output, source)
        wallet_dir.mkdir(parents=True, exist_ok=True)
        try:
            result_path = sync_source(
                source,
                wallet_dir,
                etherscan_key=os.environ.get("ETHERSCAN_API_KEY"),
                solana_rpc_url=_solana_rpc_url(),
                zerion_api_key=os.environ.get("ZERION_API_KEY"),
                mintscan_api_key=os.environ.get("MINTSCAN_API_KEY"),
                binance_api_key=os.environ.get("BINANCE_API_KEY"),
                binance_api_secret=os.environ.get("BINANCE_API_SECRET"),
                binance_max_rows=binance_max_rows,
                state_dir=state_dir,
            )
        except Exception as exc:
            label = getattr(source, "friendly_name", repr(source))
            warnings.append(f"{label}: {exc}")
            logger.exception("sync failed for %s", label)
            continue

        if result_path is None:
            label = getattr(source, "friendly_name", repr(source))
            warnings.append(f"{label}: sync returned no output (see logs)")
            continue

        files_written.append(str(result_path.relative_to(sources_dir)))

    print(json.dumps({
        "files_written": files_written,
        "records_fetched": len(files_written),
        "warnings": warnings,
    }))


if __name__ == "__main__":
    main()
