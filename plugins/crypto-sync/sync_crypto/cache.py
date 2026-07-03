"""Raw API response cache for sync providers.

Caches verbatim JSON page payloads on disk so that subsequent syncs can be
driven from local files instead of hitting the upstream API.  Each provider
has its own subdirectory; a per-collection ``index.json`` records the cache
schema version (so future format changes can invalidate or migrate older
caches), the last fetch timestamp, and the page numbers currently on disk.

Layout:

    <root>/zerion/<address>/page-NNNN.json
    <root>/zerion/<address>/index.json

    <root>/etherscan/<chain>/<address>/<action>/page-NNNN.json
    <root>/etherscan/<chain>/<address>/<action>/index.json
    <root>/etherscan/<chain>/proxy/<action>/<key>.json

    <root>/solana_rpc/signatures/<address>/page-NNNN.json
    <root>/solana_rpc/signatures/<address>/index.json
    <root>/solana_rpc/transactions/<signature>.json

EVM addresses (anything starting with ``0x``) are normalized to lowercase so
callers do not need to think about checksum casing.  Solana base58 addresses
are case-sensitive and are preserved verbatim.
"""
from __future__ import annotations

import json
import time
from pathlib import Path
from typing import Any, Iterator, Optional

CACHE_SCHEMA_VERSION = 1
ZERION_PROVIDER = "zerion"
ETHERSCAN_PROVIDER = "etherscan"
SOLANA_RPC_PROVIDER = "solana_rpc"


def _normalize_address(address: str) -> str:
    """Lowercase EVM hex addresses (0x...); preserve other formats verbatim.

    Solana base58 is case-sensitive — lowercasing would corrupt addresses.
    """
    if address[:2].lower() == "0x":
        return address.lower()
    return address


class RawResponseCache:
    """Disk-backed cache of raw API page payloads keyed by provider/address."""

    PAGE_FILENAME_TEMPLATE = "page-{:04d}.json"
    INDEX_FILENAME = "index.json"

    def __init__(self, root: Path):
        self.root = Path(root)

    # ===== Generic primitives =====

    @staticmethod
    def _read_json(path: Path) -> Optional[Any]:
        if not path.exists():
            return None
        return json.loads(path.read_text())

    @staticmethod
    def _write_json(path: Path, payload: Any, indent: Optional[int] = None) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(payload, indent=indent))

    def _read_pages(self, dir_: Path) -> Iterator[Any]:
        index = self._read_json(dir_ / self.INDEX_FILENAME)
        if not index:
            return
        for page_num in index.get("pages", []):
            payload = self._read_json(dir_ / self.PAGE_FILENAME_TEMPLATE.format(page_num))
            if payload is not None:
                yield payload

    def _write_index(self, dir_: Path, pages: list[int], **fields: Any) -> None:
        index = {
            "cache_schema_version": CACHE_SCHEMA_VERSION,
            "last_fetch_ts": int(time.time()),
            "pages": sorted(pages),
            **fields,
        }
        self._write_json(dir_ / self.INDEX_FILENAME, index, indent=2)

    # ===== Zerion =====

    def zerion_dir(self, address: str) -> Path:
        return self.root / ZERION_PROVIDER / _normalize_address(address)

    def read_zerion_page(self, address: str, page_num: int) -> Optional[dict[str, Any]]:
        return self._read_json(self.zerion_dir(address) / self.PAGE_FILENAME_TEMPLATE.format(page_num))

    def write_zerion_page(self, address: str, page_num: int, payload: dict[str, Any]) -> None:
        self._write_json(
            self.zerion_dir(address) / self.PAGE_FILENAME_TEMPLATE.format(page_num),
            payload,
        )

    def iter_zerion_pages(self, address: str) -> Iterator[dict[str, Any]]:
        yield from self._read_pages(self.zerion_dir(address))

    def read_zerion_index(self, address: str) -> Optional[dict[str, Any]]:
        return self._read_json(self.zerion_dir(address) / self.INDEX_FILENAME)

    def write_zerion_index(self, address: str, pages: list[int]) -> None:
        self._write_index(
            self.zerion_dir(address),
            pages,
            address=_normalize_address(address),
            provider=ZERION_PROVIDER,
        )

    # ===== Etherscan paginated (txlist / txlistinternal / tokentx) =====

    def etherscan_paginated_dir(self, chain_id: int, address: str, action: str) -> Path:
        return (
            self.root
            / ETHERSCAN_PROVIDER
            / str(chain_id)
            / _normalize_address(address)
            / action
        )

    def read_etherscan_page(
        self, chain_id: int, address: str, action: str, page_num: int,
    ) -> Optional[dict[str, Any]]:
        return self._read_json(
            self.etherscan_paginated_dir(chain_id, address, action)
            / self.PAGE_FILENAME_TEMPLATE.format(page_num)
        )

    def write_etherscan_page(
        self, chain_id: int, address: str, action: str, page_num: int, payload: dict[str, Any],
    ) -> None:
        self._write_json(
            self.etherscan_paginated_dir(chain_id, address, action)
            / self.PAGE_FILENAME_TEMPLATE.format(page_num),
            payload,
        )

    def iter_etherscan_pages(
        self, chain_id: int, address: str, action: str,
    ) -> Iterator[dict[str, Any]]:
        yield from self._read_pages(self.etherscan_paginated_dir(chain_id, address, action))

    def read_etherscan_index(
        self, chain_id: int, address: str, action: str,
    ) -> Optional[dict[str, Any]]:
        return self._read_json(
            self.etherscan_paginated_dir(chain_id, address, action) / self.INDEX_FILENAME
        )

    def write_etherscan_index(
        self, chain_id: int, address: str, action: str, pages: list[int],
    ) -> None:
        self._write_index(
            self.etherscan_paginated_dir(chain_id, address, action),
            pages,
            address=_normalize_address(address),
            chain_id=chain_id,
            action=action,
            provider=ETHERSCAN_PROVIDER,
        )

    # ===== Etherscan proxy (per-tx metadata) =====

    def etherscan_proxy_dir(self, chain_id: int, action: str) -> Path:
        return self.root / ETHERSCAN_PROVIDER / str(chain_id) / "proxy" / action

    def _etherscan_proxy_path(self, chain_id: int, action: str, key: str) -> Path:
        return self.etherscan_proxy_dir(chain_id, action) / f"{key}.json"

    def read_etherscan_proxy(self, chain_id: int, action: str, key: str) -> Any:
        """Return the cached proxy result, or ``None`` when the cache file is absent.

        Note: ``None`` is also returned for cached null results (e.g.
        ``eth_getTransactionByHash`` against a non-existent hash).  Use
        :meth:`has_etherscan_proxy` to disambiguate.
        """
        path = self._etherscan_proxy_path(chain_id, action, key)
        if not path.exists():
            return None
        envelope = self._read_json(path)
        return envelope.get("result") if isinstance(envelope, dict) else None

    def has_etherscan_proxy(self, chain_id: int, action: str, key: str) -> bool:
        return self._etherscan_proxy_path(chain_id, action, key).exists()

    def write_etherscan_proxy(
        self, chain_id: int, action: str, key: str, payload: Any,
    ) -> None:
        """Cache a proxy result.  Wrapped in ``{"result": ...}`` so cache-miss
        (file absent) is distinguishable from cache-hit-with-null."""
        self._write_json(
            self._etherscan_proxy_path(chain_id, action, key),
            {"result": payload},
        )

    # ===== Solana RPC: signatures (paginated) =====

    def solana_signatures_dir(self, address: str) -> Path:
        return self.root / SOLANA_RPC_PROVIDER / "signatures" / address

    def read_solana_signatures_page(
        self, address: str, page_num: int,
    ) -> Optional[dict[str, Any]]:
        return self._read_json(
            self.solana_signatures_dir(address) / self.PAGE_FILENAME_TEMPLATE.format(page_num)
        )

    def write_solana_signatures_page(
        self, address: str, page_num: int, payload: dict[str, Any],
    ) -> None:
        self._write_json(
            self.solana_signatures_dir(address) / self.PAGE_FILENAME_TEMPLATE.format(page_num),
            payload,
        )

    def iter_solana_signatures_pages(self, address: str) -> Iterator[dict[str, Any]]:
        yield from self._read_pages(self.solana_signatures_dir(address))

    def read_solana_signatures_index(self, address: str) -> Optional[dict[str, Any]]:
        return self._read_json(self.solana_signatures_dir(address) / self.INDEX_FILENAME)

    def write_solana_signatures_index(self, address: str, pages: list[int]) -> None:
        self._write_index(
            self.solana_signatures_dir(address),
            pages,
            address=address,
            provider=SOLANA_RPC_PROVIDER,
        )

    # ===== Solana RPC: per-tx blobs (keyed by signature) =====

    def solana_transactions_dir(self) -> Path:
        return self.root / SOLANA_RPC_PROVIDER / "transactions"

    def read_solana_transaction(self, signature: str) -> Optional[dict[str, Any]]:
        path = self.solana_transactions_dir() / f"{signature}.json"
        if not path.exists():
            return None
        envelope = self._read_json(path)
        return envelope.get("result") if isinstance(envelope, dict) else None

    def has_solana_transaction(self, signature: str) -> bool:
        return (self.solana_transactions_dir() / f"{signature}.json").exists()

    def write_solana_transaction(self, signature: str, payload: Optional[dict[str, Any]]) -> None:
        """Cache a getTransaction result (or ``None`` for pruned/missing).

        Wrapped in ``{"result": ...}`` so cache-miss (file absent) is
        distinguishable from cache-hit-with-null-data (file present, result
        is ``None``).
        """
        self._write_json(
            self.solana_transactions_dir() / f"{signature}.json",
            {"result": payload},
        )
