"""CLI for crypto transaction syncing."""
import argparse
import logging
import os
import sys
from pathlib import Path
from typing import Optional

from dotenv import load_dotenv

from sync_crypto.cache_orchestration import CacheMode
from sync_crypto.clients import SolanaRpcClient
from sync_crypto.sync import load_config, sync_all

logger = logging.getLogger(__name__)


def _positive_int(value: str) -> int:
    parsed = int(value)
    if parsed <= 0:
        raise argparse.ArgumentTypeError("value must be a positive integer")
    return parsed


def parse_args(argv: Optional[list[str]] = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Download crypto transaction history to CSV files.",
    )
    parser.add_argument(
        "config",
        help="Path to JSON file with wallet addresses",
    )
    parser.add_argument(
        "-o", "--output-dir",
        default="output",
        dest="output_dir",
        help="Directory for output CSV files (default: output)",
    )
    parser.add_argument(
        "--binance-max-rows",
        default=None,
        type=_positive_int,
        help=(
            "Maximum Binance accounting rows to fetch per Binance source "
            "(default: all). Can also be set via BINANCE_MAX_ROWS."
        ),
    )
    parser.add_argument(
        "--cache-dir",
        default=None,
        dest="cache_dir",
        help=(
            "Directory for the raw API response cache "
            "(default: <output-dir>/.cache/). Only used with --update-cache "
            "or --from-cache."
        ),
    )
    cache_group = parser.add_mutually_exclusive_group()
    cache_group.add_argument(
        "--update-cache",
        action="store_true",
        dest="update_cache",
        help=(
            "Hit the live API and tee each fetched page into the cache. "
            "Existing cache pages are preserved; new pages append at the "
            "next free index slot."
        ),
    )
    cache_group.add_argument(
        "--from-cache",
        action="store_true",
        dest="from_cache",
        help=(
            "Read all transactions from the cache instead of hitting any API. "
            "Wallets without cached data are skipped with a warning."
        ),
    )
    return parser.parse_args(argv)


def _resolve_cache_mode(args: argparse.Namespace) -> CacheMode:
    if args.from_cache:
        return CacheMode.READ
    if args.update_cache:
        return CacheMode.WRITE
    return CacheMode.OFF


def _resolve_cache_dir(args: argparse.Namespace, output_dir: Path) -> Optional[Path]:
    if not (args.from_cache or args.update_cache):
        return None
    if args.cache_dir:
        return Path(args.cache_dir)
    return output_dir / ".cache"


def _solana_rpc_url_from_env() -> Optional[str]:
    helius_api_key = str(os.environ.get("HELIUS_API_KEY") or "").strip()
    if helius_api_key:
        return SolanaRpcClient.resolve_rpc_url()

    explicit_url = str(os.environ.get("SOLANA_RPC_URL") or "").strip()
    if explicit_url:
        return explicit_url

    return None


def main(argv: Optional[list[str]] = None):
    logging.basicConfig(level=logging.INFO, format="%(levelname)s: %(message)s")
    load_dotenv(override=False)

    args = parse_args(argv)
    config_path = Path(args.config)

    if not config_path.exists():
        logger.error("Config file not found: %s", config_path)
        sys.exit(1)

    sources = load_config(config_path)
    output_dir = Path(args.output_dir)
    cache_mode = _resolve_cache_mode(args)
    cache_dir = _resolve_cache_dir(args, output_dir)

    etherscan_key = os.environ.get("ETHERSCAN_API_KEY")
    solana_rpc_url = _solana_rpc_url_from_env()
    zerion_api_key = os.environ.get("ZERION_API_KEY")
    mintscan_api_key = os.environ.get("MINTSCAN_API_KEY")
    binance_api_key = os.environ.get("BINANCE_API_KEY")
    binance_api_secret = os.environ.get("BINANCE_API_SECRET")
    raw_binance_max_rows = os.environ.get("BINANCE_MAX_ROWS")
    if args.binance_max_rows is not None:
        binance_max_rows = args.binance_max_rows
    elif raw_binance_max_rows:
        binance_max_rows = _positive_int(raw_binance_max_rows)
    else:
        binance_max_rows = None

    if not etherscan_key:
        logger.warning("ETHERSCAN_API_KEY not set - Ethereum syncs may fail")
    if any(getattr(source, "provider", None) == "zerion" for source in sources) and not zerion_api_key:
        logger.warning("ZERION_API_KEY not set - Zerion syncs will fail")

    results = sync_all(
        sources,
        output_dir,
        etherscan_key=etherscan_key,
        solana_rpc_url=solana_rpc_url,
        zerion_api_key=zerion_api_key,
        mintscan_api_key=mintscan_api_key,
        binance_api_key=binance_api_key,
        binance_api_secret=binance_api_secret,
        binance_max_rows=binance_max_rows,
        cache_dir=cache_dir,
        cache_mode=cache_mode,
    )

    successful = sum(1 for r in results if r is not None)
    logger.info("Synced %d/%d sources successfully", successful, len(sources))
