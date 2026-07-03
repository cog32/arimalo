#!/usr/bin/env python3
"""One-shot: build the raw API response cache for every wallet in ``wallets.json``.

Mirrors the plugin entrypoint's per-wallet ``<chain>/<address>`` output layout
but invokes ``sync_source`` with ``cache_mode=WRITE`` so each fetched API page
is teed through the cache.  Cosmos (Mintscan) wallets are skipped — not cached
in the current Phase 3 wiring.

Reads secrets from ``<vault>/plugins/crypto-sync/.data/secrets.json`` and
exports them into the environment, matching how the plugin runner sets them.

Usage:
    uv run scripts/build_cache.py [--cache-dir PATH] [--vault PATH]
"""
import argparse
import json
import logging
import os
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

DEFAULT_VAULT = Path.home() / "workspace" / "accountsv2"

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s %(levelname)s: %(message)s",
    stream=sys.stderr,
)
logger = logging.getLogger(__name__)


SECRET_TO_ENV = {
    "zerion_api_key": "ZERION_API_KEY",
    "etherscan_api_key": "ETHERSCAN_API_KEY",
    "helius_api_key": "HELIUS_API_KEY",
    "solana_rpc_url": "SOLANA_RPC_URL",
    "mintscan_api_key": "MINTSCAN_API_KEY",
    "binance_api_key": "BINANCE_API_KEY",
    "binance_api_secret": "BINANCE_API_SECRET",
}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--vault", type=Path, default=DEFAULT_VAULT)
    parser.add_argument("--cache-dir", type=Path, default=None)
    args = parser.parse_args()

    plugin_dir = args.vault / "plugins" / "crypto-sync"
    data_dir = plugin_dir / ".data"
    cache_dir = args.cache_dir or (data_dir / "cache")

    secrets_path = data_dir / "secrets.json"
    if not secrets_path.exists():
        logger.error("secrets.json not found at %s", secrets_path)
        return 2
    secrets = json.loads(secrets_path.read_text())
    for k, env_name in SECRET_TO_ENV.items():
        value = (secrets.get(k) or "").strip()
        if value:
            os.environ[env_name] = value

    from sync_crypto.cache_orchestration import CacheMode
    from sync_crypto.clients import SolanaRpcClient
    from sync_crypto.models import BinanceConfig, WalletConfig
    from sync_crypto.sync import load_config, sync_source

    wallets_file = plugin_dir / "wallets.json"
    sources = load_config(wallets_file)
    base_output = args.vault / "sources" / "richard" / "crypto" / "wallet"
    state_dir = data_dir / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    cache_dir.mkdir(parents=True, exist_ok=True)

    solana_rpc_url = (
        SolanaRpcClient.resolve_rpc_url()
        if os.environ.get("HELIUS_API_KEY")
        else (os.environ.get("SOLANA_RPC_URL") or "").strip() or None
    )

    logger.info("Cache dir: %s", cache_dir)
    logger.info("Output dir: %s", base_output)

    wallets_to_cache = [s for s in sources if isinstance(s, WalletConfig)]
    logger.info("Found %d wallet sources (%d non-wallet sources skipped)",
                len(wallets_to_cache), len(sources) - len(wallets_to_cache))

    successes = 0
    failures: list[str] = []
    skipped: list[str] = []

    for i, source in enumerate(wallets_to_cache, start=1):
        from sync_crypto.models import Blockchain
        if source.blockchain == Blockchain.COSMOS:
            logger.info("[%d/%d] Skipping Cosmos wallet %s (Mintscan not cached)",
                        i, len(wallets_to_cache), source.friendly_name)
            skipped.append(source.friendly_name)
            continue

        wallet_dir = base_output / source.blockchain.value / source.address
        wallet_dir.mkdir(parents=True, exist_ok=True)

        logger.info("[%d/%d] Caching %s (%s/%s)",
                    i, len(wallets_to_cache), source.friendly_name,
                    source.blockchain.value, source.address)
        try:
            result = sync_source(
                source,
                wallet_dir,
                etherscan_key=os.environ.get("ETHERSCAN_API_KEY"),
                solana_rpc_url=solana_rpc_url,
                zerion_api_key=os.environ.get("ZERION_API_KEY"),
                mintscan_api_key=os.environ.get("MINTSCAN_API_KEY"),
                state_dir=state_dir,
                cache_dir=cache_dir,
                cache_mode=CacheMode.WRITE,
            )
        except Exception as exc:
            logger.exception("[%d/%d] FAILED %s", i, len(wallets_to_cache), source.friendly_name)
            failures.append(f"{source.friendly_name}: {exc}")
            continue

        if result is None:
            logger.warning("[%d/%d] No output from %s", i, len(wallets_to_cache), source.friendly_name)
            failures.append(f"{source.friendly_name}: sync returned None")
            continue

        successes += 1
        logger.info("[%d/%d] DONE %s -> %s",
                    i, len(wallets_to_cache), source.friendly_name, result)

    logger.info("=" * 60)
    logger.info("Cache build complete: %d successful, %d failed, %d skipped",
                successes, len(failures), len(skipped))
    for f in failures:
        logger.info("  FAILED: %s", f)
    for s in skipped:
        logger.info("  SKIPPED: %s", s)
    return 0 if not failures else 1


if __name__ == "__main__":
    raise SystemExit(main())
