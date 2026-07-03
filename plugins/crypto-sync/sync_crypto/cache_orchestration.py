"""Cache orchestration: bridges live API clients and the disk cache.

Defines :class:`CacheMode` (OFF / READ / WRITE) plus per-provider helpers
that return page sources or substitution callables wired up for the chosen
mode.  The orchestrator (sync.py) instantiates a :class:`RawResponseCache`
once per run and calls the appropriate helper at each provider callsite.

Modes:
- ``OFF``: live API only, no cache I/O.
- ``READ``: read pages from cache, never hit the API.  Empty cache yields
  no records — callers should detect-and-skip via the ``has_*_cache`` helpers.
- ``WRITE``: hit the API and tee each page through the cache writer.  Pages
  append at the next free slot in the per-collection index, so incremental
  syncs accumulate rather than overwrite.  The index is finalised after the
  iterator exhausts; if an exception interrupts mid-fetch, the on-disk pages
  are orphaned (harmless) but absent from the index.
"""
from __future__ import annotations

from enum import Enum
from typing import Any, Callable, Iterator, Optional

from sync_crypto.cache import RawResponseCache


class CacheMode(Enum):
    OFF = "off"
    READ = "read"
    WRITE = "write"


# ===== presence checks =====

def has_zerion_cache(cache: Optional[RawResponseCache], address: str) -> bool:
    if cache is None:
        return False
    index = cache.read_zerion_index(address)
    return bool(index and index.get("pages"))


def has_etherscan_cache(
    cache: Optional[RawResponseCache], chain_id: int, address: str, action: str,
) -> bool:
    if cache is None:
        return False
    index = cache.read_etherscan_index(chain_id, address, action)
    return bool(index and index.get("pages"))


def has_solana_signatures_cache(
    cache: Optional[RawResponseCache], address: str,
) -> bool:
    if cache is None:
        return False
    index = cache.read_solana_signatures_index(address)
    return bool(index and index.get("pages"))


# ===== generic tee primitive =====

def _tee_pages_to_cache(
    pages: Iterator[Any],
    read_existing_index: Callable[[], Optional[dict]],
    write_page: Callable[[int, Any], None],
    write_index: Callable[[list[int]], None],
) -> Iterator[Any]:
    existing = read_existing_index() or {}
    existing_pages = list(existing.get("pages", []))
    next_page = (max(existing_pages) + 1) if existing_pages else 1
    new_pages: list[int] = []
    for payload in pages:
        write_page(next_page, payload)
        new_pages.append(next_page)
        next_page += 1
        yield payload
    write_index(existing_pages + new_pages)


# ===== Zerion =====

def zerion_page_source(
    cache: Optional[RawResponseCache],
    address: str,
    mode: CacheMode,
    live_factory: Callable[[], Iterator[dict[str, Any]]],
) -> Iterator[dict[str, Any]]:
    if cache is None or mode == CacheMode.OFF:
        yield from live_factory()
        return
    if mode == CacheMode.READ:
        yield from cache.iter_zerion_pages(address)
        return
    yield from _tee_pages_to_cache(
        live_factory(),
        lambda: cache.read_zerion_index(address),
        lambda n, p: cache.write_zerion_page(address, n, p),
        lambda pages: cache.write_zerion_index(address, pages),
    )


# ===== Etherscan paginated =====

def etherscan_page_source(
    cache: Optional[RawResponseCache],
    chain_id: int,
    address: str,
    action: str,
    mode: CacheMode,
    live_factory: Callable[[], Iterator[dict[str, Any]]],
) -> Iterator[dict[str, Any]]:
    if cache is None or mode == CacheMode.OFF:
        yield from live_factory()
        return
    if mode == CacheMode.READ:
        yield from cache.iter_etherscan_pages(chain_id, address, action)
        return
    yield from _tee_pages_to_cache(
        live_factory(),
        lambda: cache.read_etherscan_index(chain_id, address, action),
        lambda n, p: cache.write_etherscan_page(chain_id, address, action, n, p),
        lambda pages: cache.write_etherscan_index(chain_id, address, action, pages),
    )


# ===== Etherscan proxy (per-tx metadata) =====

def etherscan_proxy_source(
    cache: Optional[RawResponseCache],
    chain_id: int,
    mode: CacheMode,
    live_request: Callable[[str, dict], Any],
) -> Optional[Callable[[str, dict], Any]]:
    """Build a ``proxy_source`` callable for ``EtherscanClient.fetch_transaction_metadata``.

    Returns ``None`` for OFF mode (caller passes nothing → client uses the
    live API directly).  ``live_request(action, params)`` is invoked on cache
    miss in WRITE mode; it should return the unwrapped JSON-RPC result.
    """
    if cache is None or mode == CacheMode.OFF:
        return None

    def _key(params: dict) -> str:
        return params.get("txhash") or "latest"

    if mode == CacheMode.READ:
        def reader(action: str, params: dict) -> Any:
            return cache.read_etherscan_proxy(chain_id, action, _key(params))
        return reader

    def tee(action: str, params: dict) -> Any:
        key = _key(params)
        if cache.has_etherscan_proxy(chain_id, action, key):
            return cache.read_etherscan_proxy(chain_id, action, key)
        result = live_request(action, params)
        cache.write_etherscan_proxy(chain_id, action, key, result)
        return result
    return tee


# ===== Solana RPC: signatures =====

def solana_signatures_source(
    cache: Optional[RawResponseCache],
    address: str,
    mode: CacheMode,
    live_factory: Callable[[], Iterator[dict[str, Any]]],
) -> Iterator[dict[str, Any]]:
    if cache is None or mode == CacheMode.OFF:
        yield from live_factory()
        return
    if mode == CacheMode.READ:
        yield from cache.iter_solana_signatures_pages(address)
        return
    yield from _tee_pages_to_cache(
        live_factory(),
        lambda: cache.read_solana_signatures_index(address),
        lambda n, p: cache.write_solana_signatures_page(address, n, p),
        lambda pages: cache.write_solana_signatures_index(address, pages),
    )


# ===== Solana RPC: per-tx blobs =====

def solana_transaction_source(
    cache: Optional[RawResponseCache],
    mode: CacheMode,
    live_fetcher: Callable[[str], Optional[dict[str, Any]]],
) -> Optional[Callable[[str], Optional[dict[str, Any]]]]:
    """Build a ``transaction_source`` callable for ``SolanaRpcClient._fetch_transaction``.

    Returns ``None`` for OFF mode (caller passes nothing → client hits the
    RPC directly).  ``live_fetcher(signature)`` is invoked on cache miss in
    WRITE mode; it should return the raw ``getTransaction`` result (or
    ``None`` for pruned txs).
    """
    if cache is None or mode == CacheMode.OFF:
        return None
    if mode == CacheMode.READ:
        return cache.read_solana_transaction

    def tee(signature: str) -> Optional[dict[str, Any]]:
        if cache.has_solana_transaction(signature):
            return cache.read_solana_transaction(signature)
        result = live_fetcher(signature)
        cache.write_solana_transaction(signature, result)
        return result
    return tee
