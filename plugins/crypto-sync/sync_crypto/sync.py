"""Orchestration: load config, fetch transactions, write CSV."""
import csv
import json
import logging
import sys
from pathlib import Path
from typing import Any, Optional

csv.field_size_limit(sys.maxsize)

from sync_crypto.binance import sync_binance_account
from sync_crypto.cache import RawResponseCache
from sync_crypto.cache_orchestration import (
    CacheMode,
    etherscan_page_source,
    has_etherscan_cache,
    has_solana_signatures_cache,
    has_zerion_cache,
    solana_signatures_source,
    solana_transaction_source,
    zerion_page_source,
)
from sync_crypto.clients import EtherscanClient, MintscanClient, SolanaRpcClient
from sync_crypto.models import (
    BinanceConfig,
    Blockchain,
    SyncSource,
    TransactionRecord,
    WalletConfig,
    WalletHistoryProvider,
)
from sync_crypto.zerion import ZerionClient

logger = logging.getLogger(__name__)

CSV_FIELDS = [
    "record_id", "provider", "network",
    "tx_hash", "blockchain", "timestamp", "from_address", "to_address",
    "value", "fee", "status", "tx_type", "token_name", "token_symbol",
    "token_contract", "token_decimals", "block_number", "gas_used",
    "gas_price", "method", "currency", "method_id", "function_name",
    "input_data", "tx_receipt_status", "transaction_index",
    "cumulative_gas_used", "confirmations",
]


def _current_csv_header(output_path: Path) -> list[str]:
    if not output_path.exists():
        return []
    with open(output_path, newline="") as f:
        reader = csv.reader(f)
        return next(reader, [])


def _migrate_csv_schema_if_needed(output_path: Path) -> bool:
    """Upgrade an existing CSV header to current CSV_FIELDS if required."""
    if not output_path.exists():
        return False

    header = _current_csv_header(output_path)
    if header == CSV_FIELDS:
        return False

    with open(output_path, newline="") as f:
        reader = csv.DictReader(f)
        existing_rows = list(reader)

    with open(output_path, "w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=CSV_FIELDS)
        writer.writeheader()
        for row in existing_rows:
            upgraded = {field: "" for field in CSV_FIELDS}
            for key, value in row.items():
                if key in upgraded:
                    upgraded[key] = value
            writer.writerow(upgraded)

    logger.info("Upgraded CSV schema for %s", output_path)
    return True


def read_last_cursor(output_path: Path) -> dict:
    """Read the newest transaction from an existing CSV to extract sync cursor.

    Finds the row with the highest timestamp (CSV may not be sorted).
    Returns {"tx_hash": "...", "block_number": int|None} or {} if unavailable.
    """
    if not output_path.exists():
        return {}
    with open(output_path, newline="") as f:
        reader = csv.DictReader(f)
        newest_row = None
        max_ts = -1
        for row in reader:
            ts_str = row.get("timestamp", "0")
            ts = int(ts_str) if ts_str else 0
            if ts > max_ts:
                max_ts = ts
                newest_row = row
    if newest_row is None:
        return {}
    block_str = newest_row.get("block_number", "")
    return {
        "tx_hash": newest_row["tx_hash"],
        "block_number": int(block_str) if block_str else None,
    }


def _read_existing_tx_hashes(output_path: Path) -> set[str]:
    """Read stable record identifiers from an existing CSV."""
    if not output_path.exists():
        return set()
    with open(output_path, newline="") as f:
        reader = csv.DictReader(f)
        existing_ids = set()
        for row in reader:
            record_id = row.get("record_id") or row.get("tx_hash")
            if record_id:
                existing_ids.add(record_id)
        return existing_ids


def _record_key(tx: TransactionRecord) -> str:
    return tx.record_id or tx.tx_hash


def _flatten_config_data(data: Any) -> list[dict[str, Any]]:
    """Normalize legacy list and grouped-object config formats."""
    if isinstance(data, list):
        return data
    if not isinstance(data, dict):
        raise ValueError("Config must be a JSON array or object")

    items: list[dict[str, Any]] = []
    for key, raw_value in data.items():
        entries = raw_value if isinstance(raw_value, list) else [raw_value]
        for entry in entries:
            if not isinstance(entry, dict):
                raise ValueError(f"Invalid config entry under '{key}': {entry}")
            normalized = dict(entry)
            if "blockchain" not in normalized and "exchange" not in normalized:
                if str(key).strip().lower() == "binance":
                    normalized["exchange"] = "binance"
                else:
                    normalized["blockchain"] = key
            items.append(normalized)
    return items


def load_config(path: Path) -> list[SyncSource]:
    """Load wallet and exchange configuration from a JSON file."""
    with open(path) as f:
        data = json.load(f)
    normalized_items = _flatten_config_data(data)
    sources: list[SyncSource] = []
    for item in normalized_items:
        try:
            if isinstance(item, dict) and "exchange" in item:
                sources.append(BinanceConfig(**item))
            else:
                sources.append(WalletConfig(**item))
        except Exception as e:
            raise ValueError(f"Invalid wallet config: {item}") from e
    return sources


def write_csv(
    transactions: list[TransactionRecord],
    blockchain: str,
    address: str,
    output_dir: Path,
    append: bool = False,
) -> Path:
    """Write transactions to a CSV file, sorted by timestamp."""
    output_path = _csv_path(blockchain, address, output_dir)

    sorted_txs = sorted(transactions, key=lambda tx: tx.timestamp)

    if append and output_path.exists():
        _migrate_csv_schema_if_needed(output_path)
        existing_hashes = _read_existing_tx_hashes(output_path)
        sorted_txs = [tx for tx in sorted_txs if _record_key(tx) not in existing_hashes]
        with open(output_path, "a", newline="") as f:
            writer = csv.DictWriter(f, fieldnames=CSV_FIELDS)
            for tx in sorted_txs:
                writer.writerow(tx.to_csv_row())
    else:
        with open(output_path, "w", newline="") as f:
            writer = csv.DictWriter(f, fieldnames=CSV_FIELDS)
            writer.writeheader()
            for tx in sorted_txs:
                writer.writerow(tx.to_csv_row())

    logger.info("Wrote %d transactions to %s", len(sorted_txs), output_path)
    return output_path


def _csv_path(blockchain: str, address: str, output_dir: Path) -> Path:
    output_dir.mkdir(parents=True, exist_ok=True)
    return output_dir / f"{blockchain}_{address}_transactions.csv"


def _legacy_csv_path(friendly_name: str, blockchain: str, output_dir: Path) -> Path:
    output_dir.mkdir(parents=True, exist_ok=True)
    return output_dir / f"{friendly_name}_{blockchain}_transactions.csv"


def wallet_csv_path(wallet: WalletConfig, output_dir: Path) -> Path:
    """Return canonical CSV output path for a wallet."""
    return _csv_path(wallet.blockchain.value, wallet.address, output_dir)


def wallet_state_path(wallet: WalletConfig, output_dir: Path) -> Path:
    """Return the provider-specific wallet sync state path."""
    output_dir.mkdir(parents=True, exist_ok=True)
    return output_dir / f"{wallet.blockchain.value}_{wallet.address}_state.json"


def _resolve_wallet_csv_path(wallet: WalletConfig, output_dir: Path) -> Path:
    """Resolve canonical path and migrate from legacy filename if present."""
    canonical = wallet_csv_path(wallet, output_dir)
    legacy = _legacy_csv_path(wallet.friendly_name, wallet.blockchain.value, output_dir)

    if not canonical.exists() and legacy.exists():
        legacy.rename(canonical)
        logger.info("Migrated legacy wallet CSV %s -> %s", legacy, canonical)

    return canonical


def _load_wallet_state(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {}
    try:
        with open(path) as f:
            data = json.load(f)
    except (OSError, json.JSONDecodeError):
        logger.warning("Ignoring unreadable wallet sync state at %s", path)
        return {}
    return data if isinstance(data, dict) else {}


def _save_wallet_state(path: Path, state: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "w") as f:
        json.dump(state, f, indent=2, sort_keys=True)


def _write_csv_chunked(chunks, blockchain: str, address: str, output_dir: Path, append: bool = False) -> Path:
    """Write transaction chunks to CSV incrementally."""
    output_path = _csv_path(blockchain, address, output_dir)
    total = 0

    if append and output_path.exists():
        _migrate_csv_schema_if_needed(output_path)
        existing_hashes = _read_existing_tx_hashes(output_path)
        with open(output_path, "a", newline="") as f:
            writer = csv.DictWriter(f, fieldnames=CSV_FIELDS)
            for chunk in chunks:
                for tx in chunk:
                    record_id = _record_key(tx)
                    if record_id not in existing_hashes:
                        writer.writerow(tx.to_csv_row())
                        existing_hashes.add(record_id)
                        total += 1
                f.flush()
    else:
        with open(output_path, "w", newline="") as f:
            writer = csv.DictWriter(f, fieldnames=CSV_FIELDS)
            writer.writeheader()
            for chunk in chunks:
                for tx in chunk:
                    writer.writerow(tx.to_csv_row())
                total += len(chunk)
                f.flush()

    logger.info("Wrote %d transactions to %s", total, output_path)
    return output_path


def _supplement_native_eth(
    csv_path: Path,
    address: str,
    etherscan_key: str,
    cache: Optional[RawResponseCache] = None,
    cache_mode: CacheMode = CacheMode.OFF,
) -> None:
    """Append native ETH transfers from Etherscan that Zerion missed.

    A single Ethereum transaction can move both ETH (normal/internal) and
    tokens.  Zerion captures the token layer; Etherscan captures the native
    ETH layer.  Dedupe rules:

    - ``normal`` Etherscan rows are skipped if either a ``normal`` row with
      the same tx_hash already exists (hash-only — top-level txs are unique
      per hash), or a Zerion ``token_transfer`` row for native ETH (empty
      ``token_contract``) with the same tx_hash AND value already exists
      (Zerion synthesises a ``token_transfer`` for the top-level ETH
      movement of swaps/wraps/sends).
    - ``internal`` Etherscan rows are skipped if either an ``internal`` row
      with the same tx_hash AND value, or a Zerion ``token_transfer`` row for
      native ETH (empty ``token_contract``) with the same tx_hash AND value,
      already exists.  Matching on (tx_hash, value) — not hash alone —
      preserves legitimate cases where a single tx has multiple distinct
      internal ETH movements (e.g. a main transfer plus a small refund) and
      Zerion only surfaces some of them.
    """
    existing_normal_hashes: set[str] = set()
    existing_native_eth_keys: set[tuple[str, str]] = set()
    existing_internal_keys: set[tuple[str, str]] = set()
    with open(csv_path, newline="") as f:
        for row in csv.DictReader(f):
            tx_hash = row.get("tx_hash")
            if not tx_hash:
                continue
            tx_type = row.get("tx_type", "")
            value = row.get("value") or "0"
            if tx_type == "normal":
                existing_normal_hashes.add(tx_hash)
            elif tx_type == "internal":
                existing_internal_keys.add((tx_hash, value))
            elif tx_type == "token_transfer" and not row.get("token_contract"):
                existing_native_eth_keys.add((tx_hash, value))
                existing_internal_keys.add((tx_hash, value))

    client = EtherscanClient(api_key=etherscan_key)
    chain_id = client.chain_id
    native_txs: list[TransactionRecord] = []
    try:
        for action, fetch_method in (
            ("txlist", client._fetch_normal_transactions),
            ("txlistinternal", client._fetch_internal_transactions),
        ):
            live_factory = (
                lambda a=action: client._iter_paginated_pages(a, address)
            )
            page_source = etherscan_page_source(
                cache, chain_id, address, action, cache_mode, live_factory,
            )
            native_txs.extend(fetch_method(address, page_source=page_source))
    except Exception:
        logger.warning(
            "Failed to fetch native ETH transactions from Etherscan for %s",
            address,
        )
        return

    def _is_new(tx: TransactionRecord) -> bool:
        if tx.tx_type == "normal":
            if tx.tx_hash in existing_normal_hashes:
                return False
            return (tx.tx_hash, tx.value) not in existing_native_eth_keys
        if tx.tx_type == "internal":
            return (tx.tx_hash, tx.value) not in existing_internal_keys
        return True

    new_txs = [tx for tx in native_txs if _is_new(tx)]
    if not new_txs:
        return

    with open(csv_path, "a", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=CSV_FIELDS)
        for tx in new_txs:
            writer.writerow(tx.to_csv_row())

    logger.info("Supplemented %d native ETH transactions for %s", len(new_txs), address)


def sync_wallet(
    wallet: WalletConfig,
    output_dir: Path,
    etherscan_key: Optional[str] = None,
    solana_rpc_url: Optional[str] = None,
    zerion_api_key: Optional[str] = None,
    mintscan_api_key: Optional[str] = None,
    state_dir: Optional[Path] = None,
    cache_dir: Optional[Path] = None,
    cache_mode: CacheMode = CacheMode.OFF,
) -> Optional[Path]:
    """Fetch and save transactions for a single wallet. Returns None on error.

    state_dir, if given, holds wallet sync state JSON files; otherwise state
    files are written alongside the CSVs in output_dir.

    cache_dir + cache_mode wire the raw API response cache.  In WRITE mode the
    sync forces full history (start_block=0, state={}, until=None) so the cache
    builds end-to-end; in READ mode wallets without cached data are skipped.
    """
    cache = RawResponseCache(cache_dir) if cache_dir is not None else None
    try:
        csv_path = _resolve_wallet_csv_path(wallet, output_dir)
        cursor = read_last_cursor(csv_path)
        is_incremental = bool(cursor)
        state_base = state_dir if state_dir is not None else output_dir

        if wallet.provider == WalletHistoryProvider.ZERION:
            if cache_mode != CacheMode.READ and not zerion_api_key:
                logger.error(
                    "ZERION_API_KEY is required to sync Zerion wallet source %s",
                    wallet.friendly_name,
                )
                return None
            if cache_mode == CacheMode.READ and not has_zerion_cache(cache, wallet.address):
                logger.warning(
                    "Skipping Zerion wallet %s: no cached data (--from-cache)",
                    wallet.friendly_name,
                )
                return None
            if wallet.blockchain == Blockchain.SOLANA:
                logger.warning(
                    "Zerion transaction history currently excludes Solana NFT transactions; "
                    "results for %s may be partial",
                    wallet.address,
                )
            if wallet.blockchain == Blockchain.ETHEREUM and not wallet.network:
                logger.warning(
                    "Zerion wallet %s has no network filter; multichain EVM activity may be exported",
                    wallet.address,
                )

            state_path = wallet_state_path(wallet, state_base)
            state = {} if cache_mode == CacheMode.WRITE else _load_wallet_state(state_path)
            solana_rpc_client = None
            if wallet.blockchain == Blockchain.SOLANA:
                solana_rpc_client = SolanaRpcClient(rpc_url=solana_rpc_url)
            client = ZerionClient(
                api_key=zerion_api_key or "",
                solana_rpc_client=solana_rpc_client,
                etherscan_api_key=etherscan_key,
                cache=cache,
                cache_mode=cache_mode,
            )

            next_state = client.advance_state(state, [])

            def zerion_batches():
                nonlocal next_state
                live_factory = lambda: client._iter_api_pages(wallet)
                pages = zerion_page_source(cache, wallet.address, cache_mode, live_factory)
                for batch in client.fetch_new_transaction_batches(
                    wallet, state=state, page_source=pages,
                ):
                    next_state = client.advance_state(next_state, batch)
                    yield batch

            output = _write_csv_chunked(
                zerion_batches(),
                wallet.blockchain.value,
                wallet.address,
                output_dir,
                append=csv_path.exists(),
            )
            _save_wallet_state(state_path, next_state)

            # Supplement Ethereum wallets with native ETH transfers from Etherscan
            if wallet.blockchain == Blockchain.ETHEREUM and (
                etherscan_key or cache_mode == CacheMode.READ
            ):
                _supplement_native_eth(
                    output, wallet.address, etherscan_key or "",
                    cache=cache, cache_mode=cache_mode,
                )

            return output

        if wallet.blockchain == Blockchain.ETHEREUM:
            client = EtherscanClient(api_key=etherscan_key)
            chain_id = client.chain_id
            if cache_mode == CacheMode.READ and not any(
                has_etherscan_cache(cache, chain_id, wallet.address, action)
                for action in ("txlist", "txlistinternal", "tokentx")
            ):
                logger.warning(
                    "Skipping Ethereum wallet %s: no cached data (--from-cache)",
                    wallet.friendly_name,
                )
                return None

            start_block = 0 if cache_mode == CacheMode.WRITE else (cursor.get("block_number", 0) or 0)
            if cache_mode == CacheMode.OFF:
                txs = client.fetch_all_transactions(wallet.address, start_block=start_block)
            else:
                page_sources = {
                    action: etherscan_page_source(
                        cache, chain_id, wallet.address, action, cache_mode,
                        (lambda a=action, sb=start_block:
                            client._iter_paginated_pages(a, wallet.address, start_block=sb)),
                    )
                    for action in ("txlist", "txlistinternal", "tokentx")
                }
                txs = client.fetch_all_transactions(
                    wallet.address, start_block=start_block, page_sources=page_sources,
                )
            return write_csv(
                txs,
                wallet.blockchain.value,
                wallet.address,
                output_dir,
                append=is_incremental,
            )
        elif wallet.blockchain == Blockchain.SOLANA:
            if cache_mode == CacheMode.READ and not has_solana_signatures_cache(cache, wallet.address):
                logger.warning(
                    "Skipping Solana wallet %s: no cached data (--from-cache)",
                    wallet.friendly_name,
                )
                return None

            until = None if cache_mode == CacheMode.WRITE else (cursor.get("tx_hash") if is_incremental else None)
            client = SolanaRpcClient(rpc_url=solana_rpc_url)
            sigs_live = lambda: client._iter_signatures_pages(wallet.address, until=until)
            sigs_pages = solana_signatures_source(cache, wallet.address, cache_mode, sigs_live)
            tx_source = solana_transaction_source(
                cache, cache_mode,
                lambda sig: client._fetch_transaction(sig),
            )
            chunks = client.fetch_transactions_chunked(
                wallet.address,
                until=until,
                signatures_source=sigs_pages,
                transaction_source=tx_source,
            )
            return _write_csv_chunked(
                chunks,
                wallet.blockchain.value,
                wallet.address,
                output_dir,
                append=is_incremental,
            )
        elif wallet.blockchain == Blockchain.COSMOS:
            if not mintscan_api_key:
                logger.error(
                    "MINTSCAN_API_KEY is required to sync Cosmos wallet %s",
                    wallet.friendly_name,
                )
                return None
            until = cursor.get("tx_hash") if is_incremental else None
            client = MintscanClient(api_key=mintscan_api_key)
            chunks = client.fetch_transactions_chunked(wallet.address, until=until)
            return _write_csv_chunked(
                chunks,
                wallet.blockchain.value,
                wallet.address,
                output_dir,
                append=is_incremental,
            )
        else:
            logger.error("Unsupported blockchain: %s", wallet.blockchain)
            return None
    except Exception:
        logger.exception("Error syncing wallet %s (%s)", wallet.friendly_name, wallet.address)
        return None


def sync_source(
    source: SyncSource,
    output_dir: Path,
    etherscan_key: Optional[str] = None,
    solana_rpc_url: Optional[str] = None,
    zerion_api_key: Optional[str] = None,
    mintscan_api_key: Optional[str] = None,
    binance_api_key: Optional[str] = None,
    binance_api_secret: Optional[str] = None,
    binance_max_rows: Optional[int] = None,
    state_dir: Optional[Path] = None,
    cache_dir: Optional[Path] = None,
    cache_mode: CacheMode = CacheMode.OFF,
) -> Optional[Path]:
    """Dispatch sync for a wallet or Binance account source."""
    if isinstance(source, WalletConfig):
        return sync_wallet(
            source,
            output_dir,
            etherscan_key=etherscan_key,
            solana_rpc_url=solana_rpc_url,
            zerion_api_key=zerion_api_key,
            mintscan_api_key=mintscan_api_key,
            state_dir=state_dir,
            cache_dir=cache_dir,
            cache_mode=cache_mode,
        )

    if isinstance(source, BinanceConfig):
        if not binance_api_key or not binance_api_secret:
            logger.error(
                "BINANCE_API_KEY and BINANCE_API_SECRET are required to sync Binance source %s",
                source.friendly_name,
            )
            return None
        try:
            return sync_binance_account(
                source,
                output_dir,
                api_key=binance_api_key,
                api_secret=binance_api_secret,
                max_rows=binance_max_rows,
            )
        except Exception:
            logger.exception("Error syncing Binance source %s", source.friendly_name)
            return None

    logger.error("Unsupported source config: %s", source)
    return None


def sync_all(
    sources: list[SyncSource],
    output_dir: Path,
    etherscan_key: Optional[str] = None,
    solana_rpc_url: Optional[str] = None,
    zerion_api_key: Optional[str] = None,
    mintscan_api_key: Optional[str] = None,
    binance_api_key: Optional[str] = None,
    binance_api_secret: Optional[str] = None,
    binance_max_rows: Optional[int] = None,
    state_dir: Optional[Path] = None,
    cache_dir: Optional[Path] = None,
    cache_mode: CacheMode = CacheMode.OFF,
) -> list[Optional[Path]]:
    """Sync all configured sources. Errors on one source don't abort the others."""
    results = []
    for source in sources:
        result = sync_source(
            source,
            output_dir,
            etherscan_key=etherscan_key,
            solana_rpc_url=solana_rpc_url,
            zerion_api_key=zerion_api_key,
            mintscan_api_key=mintscan_api_key,
            binance_api_key=binance_api_key,
            binance_api_secret=binance_api_secret,
            binance_max_rows=binance_max_rows,
            state_dir=state_dir,
            cache_dir=cache_dir,
            cache_mode=cache_mode,
        )
        results.append(result)
    return results
