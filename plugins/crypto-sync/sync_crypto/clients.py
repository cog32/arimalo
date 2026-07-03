"""API clients for blockchain transaction fetching."""
import logging
import os
import re
import time
from datetime import datetime, timezone
from typing import Any, Callable, Iterator, Optional

import requests

from sync_crypto.models import TransactionRecord

logger = logging.getLogger(__name__)

ETHERSCAN_PAGE_SIZE = 10000
ETHERSCAN_NETWORK_CHAIN_IDS = {
    "arbitrum": 42161,
    "arbitrum-one": 42161,
    "avalanche": 43114,
    "avalanche-c": 43114,
    "avalanche-c-chain": 43114,
    "base": 8453,
    "binance-smart-chain": 56,
    "blast": 81457,
    "bsc": 56,
    "celo": 42220,
    "eth": 1,
    "ethereum": 1,
    "gnosis": 100,
    "linea": 59144,
    "matic": 137,
    "op": 10,
    "optimism": 10,
    "polygon": 137,
    "scroll": 534352,
    "xdai": 100,
    "zksync": 324,
    "zksync-era": 324,
}


class RateLimiter:
    """Simple rate limiter using sleep-based throttling."""

    def __init__(self, calls_per_second: int):
        self.min_interval = 1.0 / calls_per_second
        self.last_call: Optional[float] = None

    def wait(self):
        now = time.monotonic()
        if self.last_call is not None:
            elapsed = now - self.last_call
            if elapsed < self.min_interval:
                time.sleep(self.min_interval - elapsed)
        self.last_call = time.monotonic()


class EtherscanClient:
    """Client for Etherscan API (normal, internal, and ERC-20 token transactions)."""

    BASE_URL = "https://api.etherscan.io/v2/api"

    def __init__(self, api_key: str, chain_id: int = 1):
        self.api_key = api_key
        self.chain_id = chain_id
        self.rate_limiter = RateLimiter(calls_per_second=3)
        self._latest_block_number: Optional[int] = None

    def _request(self, params: dict) -> dict:
        self.rate_limiter.wait()
        params["apikey"] = self.api_key
        params["chainid"] = self.chain_id
        resp = requests.get(self.BASE_URL, params=params)
        resp.raise_for_status()
        return resp.json()

    @staticmethod
    def _optional_int(value: Optional[str]) -> Optional[int]:
        if value in (None, ""):
            return None
        try:
            return int(value)
        except (TypeError, ValueError):
            return None

    @staticmethod
    def _hex_to_int(value: Optional[str]) -> Optional[int]:
        if value in (None, ""):
            return None
        try:
            return int(str(value), 16)
        except (TypeError, ValueError):
            return None

    @classmethod
    def _hex_to_decimal_string(cls, value: Optional[str]) -> Optional[str]:
        parsed = cls._hex_to_int(value)
        if parsed is None:
            return None
        return str(parsed)

    @classmethod
    def chain_id_for_network(cls, network: Optional[str]) -> Optional[int]:
        """Return the Etherscan V2 chain ID for a normalized EVM network name."""
        normalized = str(network or "").strip().lower().replace("_", "-").replace(" ", "-")
        if not normalized:
            return None
        return ETHERSCAN_NETWORK_CHAIN_IDS.get(normalized)

    def _proxy_request(
        self,
        action: str,
        proxy_source: Optional[Callable[[str, dict], Any]] = None,
        **params: Any,
    ) -> Any:
        """Perform an Etherscan proxy request and return the JSON-RPC result payload.

        If ``proxy_source`` is supplied, the cached value is returned instead of
        hitting the live API.  ``proxy_source(action, params)`` should return
        the unwrapped result (or ``None`` for a cached-null result).
        """
        if proxy_source is not None:
            return proxy_source(action, dict(params))
        payload = self._request({
            "module": "proxy",
            "action": action,
            **params,
        })
        return payload.get("result")

    def _latest_block(
        self, proxy_source: Optional[Callable[[str, dict], Any]] = None,
    ) -> Optional[int]:
        """Fetch and cache the latest block number for confirmation calculations."""
        if self._latest_block_number is None:
            self._latest_block_number = self._hex_to_int(
                self._proxy_request("eth_blockNumber", proxy_source=proxy_source)
            )
        return self._latest_block_number

    def fetch_transaction_metadata(
        self,
        tx_hash: str,
        proxy_source: Optional[Callable[[str, dict], Any]] = None,
    ) -> Optional[dict[str, Any]]:
        """Fetch shared transaction execution metadata by transaction hash."""
        tx_data = self._proxy_request(
            "eth_getTransactionByHash", txhash=tx_hash, proxy_source=proxy_source,
        )
        if not isinstance(tx_data, dict):
            return None

        receipt = self._proxy_request(
            "eth_getTransactionReceipt", txhash=tx_hash, proxy_source=proxy_source,
        )
        if not isinstance(receipt, dict):
            receipt = {}

        block_number = self._hex_to_int(tx_data.get("blockNumber")) or self._hex_to_int(receipt.get("blockNumber"))
        input_data = tx_data.get("input")
        if input_data in (None, ""):
            input_data = None

        confirmations = None
        latest_block = (
            self._latest_block(proxy_source=proxy_source) if block_number is not None else None
        )
        if latest_block is not None and block_number is not None and latest_block >= block_number:
            confirmations = latest_block - block_number

        method_id = None
        if isinstance(input_data, str) and input_data.startswith("0x") and len(input_data) >= 10 and input_data != "0x":
            method_id = input_data[:10]

        return {
            "block_number": block_number,
            "gas_used": self._hex_to_decimal_string(receipt.get("gasUsed")),
            "gas_price": (
                self._hex_to_decimal_string(receipt.get("effectiveGasPrice"))
                or self._hex_to_decimal_string(tx_data.get("gasPrice"))
            ),
            "input_data": input_data,
            "tx_receipt_status": self._hex_to_decimal_string(receipt.get("status")),
            "transaction_index": (
                self._hex_to_int(tx_data.get("transactionIndex"))
                or self._hex_to_int(receipt.get("transactionIndex"))
            ),
            "cumulative_gas_used": self._hex_to_decimal_string(receipt.get("cumulativeGasUsed")),
            "confirmations": confirmations,
            "method_id": method_id,
        }

    def _normalize_normal_tx(self, raw: dict) -> TransactionRecord:
        gas_used = raw.get("gasUsed", "0")
        gas_price = raw.get("gasPrice", "0")
        fee = str(int(gas_used) * int(gas_price))
        is_error = raw.get("isError", "0")
        func_name = raw.get("functionName", "")
        method = func_name.split("(")[0] if func_name else None
        input_data = raw.get("input")
        if input_data == "deprecated":
            input_data = None

        return TransactionRecord(
            tx_hash=raw["hash"],
            blockchain="ethereum",
            timestamp=int(raw["timeStamp"]),
            from_address=raw["from"],
            to_address=raw.get("to", ""),
            value=raw["value"],
            fee=fee,
            status="failed" if is_error == "1" else "success",
            tx_type="normal",
            block_number=int(raw["blockNumber"]),
            gas_used=gas_used,
            gas_price=gas_price,
            method=method if method else None,
            currency="ETH",
            method_id=raw.get("methodId"),
            function_name=func_name or None,
            input_data=input_data,
            tx_receipt_status=raw.get("txreceipt_status"),
            transaction_index=self._optional_int(raw.get("transactionIndex")),
            cumulative_gas_used=raw.get("cumulativeGasUsed"),
            confirmations=self._optional_int(raw.get("confirmations")),
        )

    def _normalize_internal_tx(self, raw: dict) -> TransactionRecord:
        is_error = raw.get("isError", "0")
        input_data = raw.get("input")
        if input_data == "deprecated":
            input_data = None
        return TransactionRecord(
            tx_hash=raw["hash"],
            blockchain="ethereum",
            timestamp=int(raw["timeStamp"]),
            from_address=raw["from"],
            to_address=raw.get("to", ""),
            value=raw["value"],
            fee="0",
            status="failed" if is_error == "1" else "success",
            tx_type="internal",
            block_number=int(raw["blockNumber"]),
            gas_used=raw.get("gasUsed", None),
            currency="ETH",
            method_id=raw.get("methodId"),
            function_name=raw.get("functionName"),
            input_data=input_data,
            tx_receipt_status=raw.get("txreceipt_status"),
            transaction_index=self._optional_int(raw.get("transactionIndex")),
            cumulative_gas_used=raw.get("cumulativeGasUsed"),
            confirmations=self._optional_int(raw.get("confirmations")),
        )

    def _normalize_token_tx(self, raw: dict) -> TransactionRecord:
        gas_used = raw.get("gasUsed", "0")
        gas_price = raw.get("gasPrice", "0")
        fee = str(int(gas_used) * int(gas_price))
        token_symbol = (raw.get("tokenSymbol") or "").strip() or None
        token_name = (raw.get("tokenName") or "").strip() or None
        token_contract = raw.get("contractAddress")
        input_data = raw.get("input")
        if input_data == "deprecated":
            input_data = None

        return TransactionRecord(
            tx_hash=raw["hash"],
            blockchain="ethereum",
            timestamp=int(raw["timeStamp"]),
            from_address=raw["from"],
            to_address=raw.get("to", ""),
            value=raw["value"],
            fee=fee,
            status="success",
            tx_type="token_transfer",
            token_name=token_name,
            token_symbol=token_symbol,
            token_decimals=int(raw["tokenDecimal"]) if raw.get("tokenDecimal") else None,
            token_contract=token_contract,
            block_number=int(raw["blockNumber"]),
            gas_used=gas_used,
            gas_price=gas_price,
            currency=token_symbol or token_contract,
            method_id=raw.get("methodId"),
            function_name=raw.get("functionName"),
            input_data=input_data,
            tx_receipt_status=raw.get("txreceipt_status"),
            transaction_index=self._optional_int(raw.get("transactionIndex")),
            cumulative_gas_used=raw.get("cumulativeGasUsed"),
            confirmations=self._optional_int(raw.get("confirmations")),
        )

    def _iter_paginated_pages(
        self, action: str, address: str, start_block: int = 0,
    ) -> Iterator[dict[str, Any]]:
        """Yield raw API page payloads for a paginated Etherscan endpoint.

        Pure I/O — does not normalize.  The cache layer can supply an
        equivalent generator that reads payloads from disk instead.
        """
        page = 1
        while True:
            params = {
                "module": "account",
                "action": action,
                "address": address,
                "startblock": start_block,
                "endblock": 99999999,
                "page": page,
                "offset": ETHERSCAN_PAGE_SIZE,
                "sort": "asc",
            }
            payload = self._request(params)
            yield payload
            results = payload.get("result", [])
            if not isinstance(results, list) or len(results) < ETHERSCAN_PAGE_SIZE:
                break
            page += 1

    def _fetch_paginated(
        self,
        action: str,
        address: str,
        normalize_fn,
        start_block: int = 0,
        page_source: Optional[Iterator[dict[str, Any]]] = None,
    ) -> list[TransactionRecord]:
        pages = (
            page_source
            if page_source is not None
            else self._iter_paginated_pages(action, address, start_block=start_block)
        )
        all_txs: list[TransactionRecord] = []
        for payload in pages:
            results = payload.get("result", [])
            if not isinstance(results, list):
                break
            for raw in results:
                all_txs.append(normalize_fn(raw))
        return all_txs

    def _fetch_normal_transactions(
        self,
        address: str,
        start_block: int = 0,
        page_source: Optional[Iterator[dict[str, Any]]] = None,
    ) -> list[TransactionRecord]:
        return self._fetch_paginated(
            "txlist", address, self._normalize_normal_tx,
            start_block=start_block, page_source=page_source,
        )

    def _fetch_internal_transactions(
        self,
        address: str,
        start_block: int = 0,
        page_source: Optional[Iterator[dict[str, Any]]] = None,
    ) -> list[TransactionRecord]:
        return self._fetch_paginated(
            "txlistinternal", address, self._normalize_internal_tx,
            start_block=start_block, page_source=page_source,
        )

    def _fetch_token_transactions(
        self,
        address: str,
        start_block: int = 0,
        page_source: Optional[Iterator[dict[str, Any]]] = None,
    ) -> list[TransactionRecord]:
        return self._fetch_paginated(
            "tokentx", address, self._normalize_token_tx,
            start_block=start_block, page_source=page_source,
        )

    def fetch_all_transactions(
        self,
        address: str,
        start_block: int = 0,
        page_sources: Optional[dict[str, Iterator[dict[str, Any]]]] = None,
    ) -> list[TransactionRecord]:
        """Fetch all transaction types for an address.

        ``page_sources``, if supplied, maps action name (``txlist`` /
        ``txlistinternal`` / ``tokentx``) to a page-source iterator that
        substitutes the live API for that action.
        """
        sources = page_sources or {}
        logger.info("Fetching Ethereum transactions for %s", address)
        txs = []
        txs.extend(self._fetch_normal_transactions(
            address, start_block=start_block, page_source=sources.get("txlist"),
        ))
        txs.extend(self._fetch_internal_transactions(
            address, start_block=start_block, page_source=sources.get("txlistinternal"),
        ))
        txs.extend(self._fetch_token_transactions(
            address, start_block=start_block, page_source=sources.get("tokentx"),
        ))
        return txs


class SolanaRpcClient:
    """Client for Solana native JSON-RPC API (no API key required)."""

    DEFAULT_RPC_URL = "https://api.mainnet-beta.solana.com"
    SIGNATURES_LIMIT = 1000

    MAX_RETRIES = 5

    def __init__(self, rpc_url: Optional[str] = None):
        self.rpc_url = self.resolve_rpc_url(rpc_url)
        self.rate_limiter = RateLimiter(calls_per_second=5)

    @classmethod
    def resolve_rpc_url(cls, rpc_url: Optional[str] = None) -> str:
        explicit_url = str(rpc_url or "").strip()
        if explicit_url:
            return explicit_url

        helius_api_key = str(os.environ.get("HELIUS_API_KEY") or "").strip()
        if helius_api_key:
            return f"https://mainnet.helius-rpc.com/?api-key={helius_api_key}"

        env_url = str(os.environ.get("SOLANA_RPC_URL") or "").strip()
        if env_url:
            return env_url

        return cls.DEFAULT_RPC_URL

    def _rpc_call(self, method: str, params: list):
        self.rate_limiter.wait()
        payload = {
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        }
        for attempt in range(self.MAX_RETRIES):
            resp = requests.post(self.rpc_url, json=payload, timeout=30)
            if resp.status_code == 429:
                wait = 2 ** attempt + 5
                logger.warning("Rate limited (429), backing off %ds...", wait)
                time.sleep(wait)
                self.rate_limiter.last_call = time.monotonic()
                continue
            resp.raise_for_status()
            data = resp.json()
            if "error" in data:
                raise RuntimeError(f"RPC error: {data['error']}")
            return data["result"]
        resp.raise_for_status()  # Final 429 will raise here

    def _iter_signatures_pages(
        self, address: str, until: Optional[str] = None,
    ) -> Iterator[dict[str, Any]]:
        """Yield ``{"result": [...]}`` envelopes for getSignaturesForAddress.

        Pure I/O — does not normalize.  The cache layer can supply an
        equivalent generator that reads payloads from disk instead.  Wrapping
        the result list in an envelope matches the on-disk cache shape and
        keeps consumers consistent with the Etherscan/Zerion seams.
        """
        opts: dict = {"limit": self.SIGNATURES_LIMIT}
        if until:
            opts["until"] = until
        while True:
            result = self._rpc_call("getSignaturesForAddress", [address, opts])
            if not result:
                return
            yield {"result": result}
            if len(result) < self.SIGNATURES_LIMIT:
                return
            opts = {"limit": self.SIGNATURES_LIMIT, "before": result[-1]["signature"]}
            if until:
                opts["until"] = until

    def _fetch_signatures(
        self,
        address: str,
        until: Optional[str] = None,
        signatures_source: Optional[Iterator[dict[str, Any]]] = None,
    ) -> list[dict]:
        """Fetch all transaction signatures with cursor-based pagination."""
        pages = (
            signatures_source
            if signatures_source is not None
            else self._iter_signatures_pages(address, until=until)
        )
        all_sigs: list[dict] = []
        for payload in pages:
            result = payload.get("result", [])
            if not isinstance(result, list):
                break
            all_sigs.extend(result)
        return all_sigs

    def _fetch_transaction(
        self,
        signature: str,
        transaction_source: Optional[Callable[[str], Optional[dict]]] = None,
    ) -> Optional[dict]:
        """Fetch a full transaction by signature."""
        if transaction_source is not None:
            return transaction_source(signature)
        return self._rpc_call("getTransaction", [
            signature,
            {"encoding": "json", "maxSupportedTransactionVersion": 0},
        ])

    def _get_account_keys(self, tx_data: dict) -> list[str]:
        """Extract account keys, handling both legacy and versioned formats."""
        keys = tx_data["transaction"]["message"]["accountKeys"]
        if keys and isinstance(keys[0], dict):
            return [k["pubkey"] for k in keys]
        return keys

    def _parse_native_transfer(
        self, tx_data: dict, address: str, signature: str,
    ) -> Optional[TransactionRecord]:
        """Parse native SOL transfer from pre/post balance comparison."""
        meta = tx_data["meta"]
        account_keys = self._get_account_keys(tx_data)
        pre = meta["preBalances"]
        post = meta["postBalances"]
        fee = meta["fee"]

        if address not in account_keys:
            return None

        our_idx = account_keys.index(address)
        our_delta = post[our_idx] - pre[our_idx]

        # Signer (index 0) pays fee; remove fee effect to isolate transfer
        is_signer = our_idx == 0
        net_transfer = (our_delta + fee) if is_signer else our_delta

        if net_transfer == 0:
            return None

        if net_transfer > 0:
            # Received SOL - find sender (largest loss after fee adjustment)
            sender = address
            max_loss = 0
            for i, key in enumerate(account_keys):
                delta = post[i] - pre[i]
                if i == 0:
                    delta += fee
                if delta < 0 and abs(delta) > max_loss:
                    max_loss = abs(delta)
                    sender = key
            from_addr, to_addr = sender, address
            value = str(net_transfer)
        else:
            # Sent SOL - find receiver (largest gain)
            receiver = address
            max_gain = 0
            for i, key in enumerate(account_keys):
                delta = post[i] - pre[i]
                if delta > 0 and delta > max_gain:
                    max_gain = delta
                    receiver = key
            from_addr, to_addr = address, receiver
            value = str(abs(net_transfer))

        return TransactionRecord(
            tx_hash=signature,
            blockchain="solana",
            timestamp=tx_data["blockTime"],
            from_address=from_addr,
            to_address=to_addr,
            value=value,
            fee=str(fee),
            status="success",
            tx_type="native",
            block_number=tx_data["slot"],
            currency="SOL",
        )

    def _parse_token_transfers(
        self, tx_data: dict, address: str, signature: str,
    ) -> list[TransactionRecord]:
        """Parse SPL token transfers from pre/post token balance comparison."""
        meta = tx_data["meta"]
        pre_tokens = meta.get("preTokenBalances", [])
        post_tokens = meta.get("postTokenBalances", [])
        fee = meta["fee"]

        def build_balances(token_balances):
            balances = {}
            for entry in token_balances:
                owner = entry.get("owner", "")
                mint = entry.get("mint", "")
                amount = int(entry["uiTokenAmount"]["amount"])
                decimals = entry["uiTokenAmount"]["decimals"]
                balances[(mint, owner)] = (amount, decimals)
            return balances

        pre_bal = build_balances(pre_tokens)
        post_bal = build_balances(post_tokens)

        all_keys = set(pre_bal.keys()) | set(post_bal.keys())
        mints = {mint for mint, _ in all_keys}

        records = []
        for mint in mints:
            owners = {owner for m, owner in all_keys if m == mint}
            if address not in owners:
                continue

            deltas = {}
            decimals = None
            for owner in owners:
                pre_amount, pre_dec = pre_bal.get((mint, owner), (0, 0))
                post_amount, post_dec = post_bal.get((mint, owner), (0, 0))
                deltas[owner] = post_amount - pre_amount
                if decimals is None:
                    decimals = pre_dec or post_dec

            our_delta = deltas.get(address, 0)
            if our_delta == 0:
                continue

            if our_delta > 0:
                losers = [o for o in owners if deltas.get(o, 0) < 0]
                sender = max(losers, key=lambda o: abs(deltas[o]), default="") if losers else ""
                from_addr, to_addr = sender, address
                value = str(our_delta)
            else:
                gainers = [o for o in owners if deltas.get(o, 0) > 0]
                receiver = max(gainers, key=lambda o: deltas[o], default="") if gainers else ""
                from_addr, to_addr = address, receiver
                value = str(abs(our_delta))

            records.append(TransactionRecord(
                tx_hash=signature,
                blockchain="solana",
                timestamp=tx_data["blockTime"],
                from_address=from_addr,
                to_address=to_addr,
                value=value,
                fee=str(fee),
                status="success",
                tx_type="token_transfer",
                token_contract=mint,
                token_decimals=decimals,
                block_number=tx_data["slot"],
                currency=mint,
            ))

        return records

    def _parse_transaction(
        self, tx_data: Optional[dict], address: str, signature: str,
    ) -> list[TransactionRecord]:
        """Parse a transaction into one or more TransactionRecords."""
        if tx_data is None:
            return []

        meta = tx_data.get("meta")
        if meta is None:
            return []

        if meta["err"] is not None:
            account_keys = self._get_account_keys(tx_data)
            return [TransactionRecord(
                tx_hash=signature,
                blockchain="solana",
                timestamp=tx_data["blockTime"],
                from_address=account_keys[0] if account_keys else "",
                to_address="",
                value="0",
                fee=str(meta["fee"]),
                status="failed",
                tx_type="native",
                block_number=tx_data["slot"],
                currency="SOL",
            )]

        records = []
        records.extend(self._parse_token_transfers(tx_data, address, signature))
        native = self._parse_native_transfer(tx_data, address, signature)
        if native:
            records.append(native)
        return records

    CHUNK_SIZE = 25

    def fetch_transactions_chunked(
        self,
        address: str,
        until: Optional[str] = None,
        signatures_source: Optional[Iterator[dict[str, Any]]] = None,
        transaction_source: Optional[Callable[[str], Optional[dict]]] = None,
    ):
        """Yield chunks of TransactionRecords, skipping individual failures."""
        logger.info("Fetching Solana transactions for %s", address)
        signatures = self._fetch_signatures(
            address, until=until, signatures_source=signatures_source,
        )
        logger.info("Found %d signatures for %s", len(signatures), address)

        for i in range(0, len(signatures), self.CHUNK_SIZE):
            chunk_sigs = signatures[i:i + self.CHUNK_SIZE]
            chunk_txs = []
            for sig_info in chunk_sigs:
                sig = sig_info["signature"]
                try:
                    tx_data = self._fetch_transaction(sig, transaction_source=transaction_source)
                    records = self._parse_transaction(tx_data, address, sig)
                    chunk_txs.extend(records)
                except Exception:
                    logger.warning("Failed to fetch tx %s, skipping", sig)
            if chunk_txs:
                logger.info("Fetched chunk %d-%d (%d txs)",
                            i + 1, min(i + self.CHUNK_SIZE, len(signatures)), len(chunk_txs))
                yield chunk_txs


class MintscanClient:
    """Client for Mintscan API (Cosmos SDK chain transaction fetching)."""

    BASE_URL = "https://apis.mintscan.io"
    PAGE_SIZE = 20
    MAX_RETRIES = 5

    NETWORK_BY_PREFIX = {
        "osmo": "osmosis",
        "cosmos": "cosmos",
        "inj": "injective",
        "celestia": "celestia",
        "axelar": "axelar",
    }

    _AMOUNT_RE = re.compile(r"^(\d+)(.*)")

    def __init__(self, api_key: str):
        self.api_key = api_key
        self.rate_limiter = RateLimiter(calls_per_second=2)

    def _network_for_address(self, address: str) -> str:
        for prefix, network in self.NETWORK_BY_PREFIX.items():
            if address.startswith(prefix):
                return network
        raise ValueError(f"Unknown Cosmos address prefix: {address}")

    @staticmethod
    def _parse_amount_string(raw: str) -> tuple[str, str]:
        """Parse '1709046927uosmo' into ('1709046927', 'uosmo')."""
        if not raw:
            return ("0", "")
        m = MintscanClient._AMOUNT_RE.match(raw)
        if m:
            return (m.group(1), m.group(2))
        return ("0", raw)

    def _request(self, path: str, params: Optional[dict] = None) -> dict:
        self.rate_limiter.wait()
        url = f"{self.BASE_URL}{path}"
        headers = {"Authorization": f"Bearer {self.api_key}"}
        for attempt in range(self.MAX_RETRIES):
            resp = requests.get(url, params=params, headers=headers, timeout=30)
            if resp.status_code == 429:
                wait = 2 ** attempt + 5
                logger.warning("Rate limited (429), backing off %ds...", wait)
                time.sleep(wait)
                self.rate_limiter.last_call = time.monotonic()
                continue
            resp.raise_for_status()
            return resp.json()
        resp.raise_for_status()

    @staticmethod
    def _parse_iso_timestamp(ts: str) -> int:
        """Parse ISO 8601 timestamp to Unix seconds."""
        # Handle both 'Z' suffix and '+00:00'
        ts = ts.replace("Z", "+00:00")
        dt = datetime.fromisoformat(ts)
        return int(dt.replace(tzinfo=timezone.utc).timestamp()) if dt.tzinfo is None else int(dt.timestamp())

    def _extract_fee(self, raw_tx: dict) -> str:
        """Extract fee amount from tx auth_info."""
        tx_inner = raw_tx.get("tx", {}).get("/cosmos-tx-v1beta1-Tx", {})
        fee_amounts = tx_inner.get("auth_info", {}).get("fee", {}).get("amount", [])
        if fee_amounts:
            return fee_amounts[0].get("amount", "0")
        return "0"

    def _normalize_transaction(self, raw_tx: dict, address: str) -> list[TransactionRecord]:
        """Convert a Mintscan transaction to TransactionRecord(s).

        One record per transfer event involving the target address.
        For failed txs with no transfer events, one record is still emitted.
        """
        tx_hash = raw_tx["txhash"]
        code = raw_tx.get("code", 0)
        status = "success" if code == 0 else "failed"
        timestamp = self._parse_iso_timestamp(raw_tx["timestamp"])
        fee = self._extract_fee(raw_tx)
        block_number = int(raw_tx["height"])
        gas_used = raw_tx.get("gas_used", "0")

        # Extract transfer events involving our address
        records = []
        logs = raw_tx.get("logs", [])
        for log_entry in logs:
            for event in log_entry.get("events", []):
                if event["type"] != "transfer":
                    continue
                attrs = {}
                for attr in event.get("attributes", []):
                    if attr["key"] in ("recipient", "sender", "amount"):
                        attrs[attr["key"]] = attr["value"]
                sender = attrs.get("sender", "")
                recipient = attrs.get("recipient", "")
                if address not in (sender, recipient):
                    continue
                amount_str = attrs.get("amount", "0")
                value, denom = self._parse_amount_string(amount_str)
                records.append(TransactionRecord(
                    tx_hash=tx_hash,
                    blockchain="cosmos",
                    timestamp=timestamp,
                    from_address=sender,
                    to_address=recipient,
                    value=value,
                    fee=fee,
                    status=status,
                    tx_type="transfer",
                    block_number=block_number,
                    gas_used=gas_used,
                    currency=denom,
                ))

        # For failed txs or txs with no matching transfer events that involve
        # our address as a signer, still emit a record so the tx is tracked.
        if not records and status == "failed":
            # Try to extract from/to from message
            tx_inner = raw_tx.get("tx", {}).get("/cosmos-tx-v1beta1-Tx", {})
            msgs = tx_inner.get("body", {}).get("messages", [])
            from_addr = ""
            to_addr = ""
            if msgs:
                msg = msgs[0]
                inner_key = [k for k in msg if k != "@type"]
                if inner_key:
                    inner = msg.get(inner_key[0], {})
                    if isinstance(inner, dict):
                        from_addr = inner.get("from_address", inner.get("sender", ""))
                        to_addr = inner.get("to_address", inner.get("contract", ""))
            records.append(TransactionRecord(
                tx_hash=tx_hash,
                blockchain="cosmos",
                timestamp=timestamp,
                from_address=from_addr,
                to_address=to_addr,
                value="0",
                fee=fee,
                status=status,
                block_number=block_number,
                gas_used=gas_used,
                currency="",
            ))

        return records

    def fetch_transactions_chunked(
        self, address: str, until: Optional[str] = None,
    ):
        """Generator yielding chunks of TransactionRecords for a Cosmos address.

        Uses Mintscan searchAfter cursor pagination. Stops when reaching
        the `until` tx hash (for incremental sync) or when no more pages.
        """
        network = self._network_for_address(address)
        path = f"/v1/{network}/accounts/{address}/transactions"
        search_after = None

        while True:
            params: dict = {"take": self.PAGE_SIZE, "fromDateTime": "2020-01-01T00:00:00Z"}
            if search_after:
                params["searchAfter"] = search_after

            data = self._request(path, params=params)
            txs = data.get("transactions", [])
            if not txs:
                break

            chunk = []
            hit_until = False
            for raw_tx in txs:
                if until and raw_tx["txhash"] == until:
                    hit_until = True
                    break
                records = self._normalize_transaction(raw_tx, address)
                chunk.extend(records)

            if chunk:
                yield chunk
            if hit_until:
                break

            pagination = data.get("pagination", {})
            search_after = pagination.get("searchAfter")
            if not search_after:
                break
