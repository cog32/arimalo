"""Zerion wallet history client."""
from __future__ import annotations

import datetime as dt
import json
import logging
import time
from decimal import Decimal, InvalidOperation
from pathlib import Path
from typing import Any, Iterator, Optional
from urllib.parse import parse_qsl, urlparse

import requests
from requests.auth import HTTPBasicAuth

from sync_crypto.cache import RawResponseCache
from sync_crypto.cache_orchestration import CacheMode, etherscan_proxy_source
from sync_crypto.clients import EtherscanClient, RateLimiter, SolanaRpcClient
from sync_crypto.models import Blockchain, TransactionRecord, WalletConfig

logger = logging.getLogger(__name__)


class ZerionClient:
    """Client for Zerion wallet transaction history."""

    BASE_URL = "https://api.zerion.io/v1"
    DEFAULT_PAGE_SIZE = 100
    MIN_PAGE_SIZE = 10
    MAX_RETRIES = 5
    WRAPPED_SOL_MINT = "So11111111111111111111111111111111111111112"
    SOLANA_STANDARD_PROGRAM_IDS = {
        "11111111111111111111111111111111",
        "ComputeBudget111111111111111111111111111111",
        "SysvarRent111111111111111111111111111111111",
        "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
        "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
    }

    def __init__(
        self,
        api_key: str,
        page_size: int = DEFAULT_PAGE_SIZE,
        solana_rpc_client: Optional[SolanaRpcClient] = None,
        etherscan_api_key: Optional[str] = None,
        cache: Optional[RawResponseCache] = None,
        cache_mode: CacheMode = CacheMode.OFF,
    ):
        self.api_key = api_key
        self.page_size = page_size
        self.rate_limiter = RateLimiter(calls_per_second=2)
        self.solana_rpc_client = solana_rpc_client
        self.etherscan_api_key = etherscan_api_key
        self.cache = cache
        self.cache_mode = cache_mode
        self._etherscan_clients: dict[int, EtherscanClient] = {}
        self._solana_mint_labels: Optional[dict[str, str]] = None

    def _request(self, url: str, params: Optional[dict[str, Any]] = None) -> dict[str, Any]:
        request_params = dict(params or {})
        last_response: Optional[requests.Response] = None
        for attempt in range(self.MAX_RETRIES):
            self.rate_limiter.wait()
            resp = requests.get(
                url,
                params=dict(request_params),
                auth=HTTPBasicAuth(self.api_key, ""),
                headers={"Accept": "application/json"},
                timeout=30,
            )
            last_response = resp
            if resp.status_code == 429:
                wait = 2 ** attempt + 5
                logger.warning("Zerion rate limited (429), backing off %ds...", wait)
                time.sleep(wait)
                self.rate_limiter.last_call = time.monotonic()
                continue
            if 500 <= resp.status_code < 600:
                reduced_page_size = self._reduce_page_size(request_params)
                if reduced_page_size is not None:
                    logger.warning(
                        "Zerion server error (%s), retrying with smaller page size %s",
                        resp.status_code,
                        reduced_page_size,
                    )
                    self.rate_limiter.last_call = time.monotonic()
                    continue
                wait = 2 ** attempt + 5
                logger.warning("Zerion server error (%s), backing off %ds...", resp.status_code, wait)
                time.sleep(wait)
                self.rate_limiter.last_call = time.monotonic()
                continue
            resp.raise_for_status()
            return resp.json()
        if last_response is not None:
            last_response.raise_for_status()
        return {}

    @classmethod
    def _reduce_page_size(cls, params: dict[str, Any]) -> Optional[int]:
        raw_value = params.get("page[size]")
        if raw_value in (None, ""):
            return None
        try:
            current_page_size = int(raw_value)
        except (TypeError, ValueError):
            return None
        if current_page_size <= cls.MIN_PAGE_SIZE:
            return None

        next_page_size = max(cls.MIN_PAGE_SIZE, current_page_size // 2)
        if next_page_size >= current_page_size:
            return None

        params["page[size]"] = next_page_size
        return next_page_size

    def _initial_params(self, wallet: WalletConfig) -> dict[str, Any]:
        params: dict[str, Any] = {"page[size]": self.page_size}
        chain_ids: list[str] = []
        if wallet.network:
            chain_ids.append(wallet.network)
        elif wallet.blockchain == Blockchain.SOLANA:
            chain_ids.append(Blockchain.SOLANA.value)
        if chain_ids:
            params["filter[chain_ids]"] = ",".join(chain_ids)
        return params

    @staticmethod
    def _params_from_next_url(next_url: str) -> tuple[str, dict[str, str]]:
        parsed = urlparse(next_url)
        params = dict(parse_qsl(parsed.query, keep_blank_values=True))
        clean_url = parsed._replace(query="").geturl()
        return clean_url, params

    @staticmethod
    def _parse_timestamp(value: Any) -> int:
        if value in (None, ""):
            return 0
        if isinstance(value, (int, float)):
            return int(value)
        text = str(value).strip()
        if not text:
            return 0
        try:
            if text.endswith("Z"):
                text = text[:-1] + "+00:00"
            return int(dt.datetime.fromisoformat(text).timestamp())
        except ValueError:
            return 0

    @staticmethod
    def _first_present(*values: Any) -> Optional[str]:
        for value in values:
            if isinstance(value, dict):
                nested = value.get("address") or value.get("hash") or value.get("id")
                if nested:
                    return str(nested)
            elif value not in (None, ""):
                return str(value)
        return None

    @staticmethod
    def _extract_quantity_components(container: Any) -> tuple[Optional[str], Optional[str], Optional[int]]:
        if not isinstance(container, dict):
            return None, None, None
        quantity = container.get("quantity")
        if not isinstance(quantity, dict):
            quantity = container
        raw_value = quantity.get("value") or quantity.get("raw") or quantity.get("integer")
        numeric = quantity.get("numeric") or quantity.get("float")
        decimals = quantity.get("decimals")
        try:
            parsed_decimals = int(decimals) if decimals not in (None, "") else None
        except (TypeError, ValueError):
            parsed_decimals = None
        return (
            str(raw_value) if raw_value not in (None, "") else None,
            str(numeric) if numeric not in (None, "") else None,
            parsed_decimals,
        )

    @staticmethod
    def _to_base_units(raw_value: Optional[str], numeric: Optional[str], decimals: Optional[int]) -> str:
        if raw_value not in (None, ""):
            return str(raw_value)
        if numeric in (None, ""):
            return "0"
        if decimals is None:
            return str(numeric)
        try:
            scaled = Decimal(str(numeric)) * (Decimal(10) ** decimals)
            return str(int(scaled))
        except (InvalidOperation, ValueError):
            return str(numeric)

    @staticmethod
    def _normalize_status(value: Any) -> str:
        status = str(value or "").strip().lower()
        if status in {"confirmed", "completed", "success", "succeeded"}:
            return "success"
        if status in {"failed", "reverted", "error"}:
            return "failed"
        return status or "unknown"

    @staticmethod
    def _extract_chain_id(item: dict[str, Any], transfer: Optional[dict[str, Any]] = None) -> Optional[str]:
        if isinstance(transfer, dict):
            chain_id = transfer.get("chain_id") or transfer.get("network")
            if chain_id:
                return str(chain_id)
        relationships = item.get("relationships") or {}
        chain_data = ((relationships.get("chain") or {}).get("data") or {})
        chain_id = chain_data.get("id")
        if chain_id:
            return str(chain_id)
        attributes = item.get("attributes") or {}
        chain_id = attributes.get("chain_id") or attributes.get("network")
        if chain_id:
            return str(chain_id)
        return None

    @staticmethod
    def _extract_fee(attributes: dict[str, Any]) -> str:
        fee = attributes.get("fee")
        raw_value, numeric, decimals = ZerionClient._extract_quantity_components(fee)
        return ZerionClient._to_base_units(raw_value, numeric, decimals)

    @staticmethod
    def _blockchain_for(wallet: WalletConfig, chain_id: Optional[str]) -> str:
        if chain_id == Blockchain.SOLANA.value or wallet.blockchain == Blockchain.SOLANA:
            return Blockchain.SOLANA.value
        return Blockchain.ETHEREUM.value

    @staticmethod
    def _select_implementation(
        implementations: list[dict[str, Any]],
        preferred_chain_id: Optional[str],
    ) -> dict[str, Any]:
        normalized_preferred = str(preferred_chain_id or "").strip().lower()
        if normalized_preferred:
            for implementation in implementations:
                if not isinstance(implementation, dict):
                    continue
                chain_id = str(implementation.get("chain_id") or "").strip().lower()
                if chain_id == normalized_preferred:
                    return implementation
        return implementations[0] if implementations else {}

    @staticmethod
    def _extract_method_metadata(attributes: dict[str, Any]) -> tuple[Optional[str], Optional[str]]:
        application_metadata = attributes.get("application_metadata") or {}
        method = application_metadata.get("method") or {}
        method_id = method.get("id")
        function_name = method.get("name")
        return (
            str(method_id) if method_id not in (None, "") else None,
            str(function_name) if function_name not in (None, "") else None,
        )

    @staticmethod
    def _extract_spl_token_amount(info: dict[str, Any]) -> Optional[str]:
        amount = info.get("amount")
        if amount not in (None, ""):
            return str(amount)
        token_amount = info.get("tokenAmount") or {}
        nested_amount = token_amount.get("amount")
        if nested_amount not in (None, ""):
            return str(nested_amount)
        return None

    @staticmethod
    def _extract_account_keys(tx_data: dict[str, Any]) -> list[str]:
        keys = ((tx_data.get("transaction") or {}).get("message") or {}).get("accountKeys") or []
        if keys and isinstance(keys[0], dict):
            return [str(key.get("pubkey") or "") for key in keys]
        return [str(key) for key in keys]

    @classmethod
    def _resolve_instruction_accounts(
        cls,
        instruction: dict[str, Any],
        account_keys: list[str],
    ) -> list[str]:
        accounts = instruction.get("accounts") or []
        resolved: list[str] = []
        for account in accounts:
            if isinstance(account, int):
                if 0 <= account < len(account_keys):
                    resolved.append(account_keys[account])
            elif isinstance(account, dict):
                pubkey = account.get("pubkey")
                if pubkey not in (None, ""):
                    resolved.append(str(pubkey))
            elif account not in (None, ""):
                resolved.append(str(account))
        return resolved

    @classmethod
    def _extract_outer_program_id(
        cls,
        instruction: dict[str, Any],
        account_keys: list[str],
    ) -> Optional[str]:
        program_id = instruction.get("programId")
        if program_id not in (None, ""):
            return str(program_id)
        program_id_index = instruction.get("programIdIndex")
        if isinstance(program_id_index, int) and 0 <= program_id_index < len(account_keys):
            return account_keys[program_id_index]
        return None

    @classmethod
    def _drop_self_owned_wsol_ata_close_receives(
        cls,
        base_records: list[TransactionRecord],
        tx_data: dict[str, Any],
        wallet_address: str,
    ) -> list[TransactionRecord]:
        """Drop Zerion base SOL receives whose source is a wSOL token
        account being closed by the wallet in this tx. closeAccount returns
        the ATA's lamports to the wallet, but the wSOL it held was already
        credited at the swap that funded the ATA — emitting the close as a
        fresh receive double-credits the wallet's SOL balance."""
        closed_atas = cls._self_owned_wsol_close_accounts(tx_data, wallet_address)
        if not closed_atas:
            return base_records
        return [
            r for r in base_records
            if not (
                r.token_symbol == "SOL"
                and not r.token_contract
                and r.from_address in closed_atas
                and r.to_address == wallet_address
            )
        ]

    @classmethod
    def _self_owned_wsol_close_accounts(
        cls,
        tx_data: dict[str, Any],
        wallet_address: str,
    ) -> set[str]:
        """Addresses of wSOL token accounts being closed by the wallet in
        this tx — closeAccount where owner==wallet, destination==wallet,
        on an account that held wSOL pre-tx. Used to filter the phantom
        SOL receive Zerion synthesizes for these close-account events."""
        instructions = (
            ((tx_data.get("transaction") or {}).get("message") or {}).get("instructions")
            or []
        )
        candidates: set[str] = set()
        for ix in instructions:
            if ix.get("program") != "spl-token":
                continue
            parsed = ix.get("parsed") or {}
            if parsed.get("type") != "closeAccount":
                continue
            info = parsed.get("info") or {}
            if (
                info.get("owner") == wallet_address
                and info.get("destination") == wallet_address
                and info.get("account")
            ):
                candidates.add(str(info["account"]))
        if not candidates:
            return set()

        meta = tx_data.get("meta") or {}
        pre_balances = meta.get("preTokenBalances") or []
        account_keys = cls._extract_account_keys(tx_data)
        wsol_closes: set[str] = set()
        for b in pre_balances:
            if b.get("mint") != cls.WRAPPED_SOL_MINT:
                continue
            idx = b.get("accountIndex")
            if isinstance(idx, int) and 0 <= idx < len(account_keys):
                addr = account_keys[idx]
                if addr in candidates:
                    wsol_closes.add(addr)
        return wsol_closes

    @classmethod
    def _build_token_account_state(cls, tx_data: dict[str, Any]) -> dict[str, dict[str, Any]]:
        meta = tx_data.get("meta") or {}
        account_keys = cls._extract_account_keys(tx_data)
        state: dict[str, dict[str, Any]] = {}

        def apply_entries(entries: list[dict[str, Any]], phase: str) -> None:
            for entry in entries:
                if not isinstance(entry, dict):
                    continue
                account_index = entry.get("accountIndex")
                if not isinstance(account_index, int) or not (0 <= account_index < len(account_keys)):
                    continue
                token_account = account_keys[account_index]
                token_state = state.setdefault(
                    token_account,
                    {
                        "mint": entry.get("mint"),
                        "decimals": None,
                        "owner": entry.get("owner"),
                        "pre": 0,
                        "post": 0,
                    },
                )
                token_state["mint"] = entry.get("mint") or token_state.get("mint")
                token_state["owner"] = entry.get("owner") or token_state.get("owner")
                ui_token_amount = entry.get("uiTokenAmount") or {}
                decimals = ui_token_amount.get("decimals")
                if decimals not in (None, ""):
                    try:
                        token_state["decimals"] = int(decimals)
                    except (TypeError, ValueError):
                        pass
                amount = ui_token_amount.get("amount")
                try:
                    parsed_amount = int(amount) if amount not in (None, "") else 0
                except (TypeError, ValueError):
                    parsed_amount = 0
                token_state[phase] = parsed_amount

        apply_entries(meta.get("preTokenBalances") or [], "pre")
        apply_entries(meta.get("postTokenBalances") or [], "post")

        for token_state in state.values():
            token_state["delta"] = int(token_state.get("post") or 0) - int(token_state.get("pre") or 0)

        return state

    # Direction-words Zerion uses for "value flowing into this record's
    # primary side" — `deposit` for bridge ins, `mint` for wrapped-token /
    # liquid-staking mints, `trade` for the buy leg of a swap, plain
    # `receive` for transfers. Synthesizers downstream all emit `receive`,
    # so we register a "receive" alias for any of these to keep the
    # fingerprint dedup direction-word agnostic.
    _INBOUND_METHODS = frozenset({"receive", "deposit", "mint", "trade"})

    @classmethod
    def _existing_record_fingerprints(
        cls,
        wallet_address: str,
        records: list[TransactionRecord],
    ) -> set[tuple[str, str, str]]:
        fingerprints: set[tuple[str, str, str]] = set()
        for record in records:
            asset_id = str(record.token_contract or record.token_symbol or record.currency or "")
            if record.from_address == wallet_address and record.to_address != wallet_address:
                direction = "send"
            elif record.to_address == wallet_address and record.from_address != wallet_address:
                direction = "receive"
            else:
                direction = str(record.method or "")
            fingerprints.add((asset_id, str(record.value), direction))
            # Wormhole/LST bridge-ins have from==to==wallet and Zerion labels
            # them `deposit` / `mint`; a synthetic mint-receive for the same
            # (mint, value) would otherwise slip past the dedup. Add an alias
            # under the canonical "receive" direction.
            if direction in cls._INBOUND_METHODS:
                fingerprints.add((asset_id, str(record.value), "receive"))
            if (
                record.blockchain == Blockchain.SOLANA.value
                and record.tx_type == "token_transfer"
                and not record.token_contract
                and (record.token_symbol == "SOL" or record.currency == "SOL")
            ):
                fingerprints.add((cls.WRAPPED_SOL_MINT, str(record.value), direction))
                if direction in cls._INBOUND_METHODS:
                    fingerprints.add((cls.WRAPPED_SOL_MINT, str(record.value), "receive"))
                # The rpc enrichment derives WSOL_MINT receives/sends from
                # gross pre/post token-balance deltas, while Zerion's base
                # SOL row carries a net amount. The two routinely differ by
                # the swap program's transient ATA rent (~0.00275 SOL), so
                # exact-value dedup misses them and the same on-chain SOL
                # flow gets credited twice. Register a wildcard so any
                # WSOL_MINT row in the same direction on this tx is treated
                # as a duplicate of the base SOL row.
                fingerprints.add((cls.WRAPPED_SOL_MINT, cls._ANY_VALUE, direction))
                if direction in cls._INBOUND_METHODS:
                    fingerprints.add((cls.WRAPPED_SOL_MINT, cls._ANY_VALUE, "receive"))
                # Cross-direction wrap/unwrap mirror. Wrapping the wallet's
                # OWN SOL books a native `send` (lamports leave the main
                # account) that the rpc enrichment mirrors as a same-value
                # WSOL `receive` (the wallet's wSOL ATA fills) on the same tx;
                # unwrapping is the reverse. These mirrors are not external
                # transfers, so register the opposite-direction WSOL
                # fingerprint at the exact value to dedupe the synthetic leg.
                # (Port Finance pSOL deposits wrapped SOL and were credited
                # twice — +385 phantom SOL on 3FXQ — because the existing
                # same-direction dedup above never matched the wrap.)
                if direction == "send":
                    fingerprints.add((cls.WRAPPED_SOL_MINT, str(record.value), "receive"))
                elif direction == "receive":
                    fingerprints.add((cls.WRAPPED_SOL_MINT, str(record.value), "send"))
        return fingerprints

    # Sentinel used as the value field of a fingerprint to mean "any value
    # for this (mint, direction) pair on this tx". Only set for WSOL_MINT
    # when an inbound/outbound base SOL record exists — see
    # `_existing_record_fingerprints` for the rationale.
    _ANY_VALUE = "*"

    @classmethod
    def _is_wsol_duplicate_of_base_sol(
        cls,
        mint: str,
        direction: str,
        existing_fingerprints: set[tuple[str, str, str]],
    ) -> bool:
        return (
            mint == cls.WRAPPED_SOL_MINT
            and (mint, cls._ANY_VALUE, direction) in existing_fingerprints
        )

    @classmethod
    def _record_direction(
        cls,
        wallet_address: str,
        record: TransactionRecord,
    ) -> str:
        if record.from_address == wallet_address and record.to_address != wallet_address:
            return "send"
        if record.to_address == wallet_address and record.from_address != wallet_address:
            return "receive"
        return str(record.method or "")

    @classmethod
    def _parsed_transfer_direction(
        cls,
        wallet_address: str,
        transfer: dict[str, Any],
        destination_owner: Optional[str],
    ) -> Optional[str]:
        if transfer.get("authority") == wallet_address:
            return "send"
        if destination_owner == wallet_address or transfer.get("destination") == wallet_address:
            return "receive"
        return None

    @classmethod
    def _upgrade_solana_base_records(
        cls,
        wallet_address: str,
        base_records: list[TransactionRecord],
        parsed_transfers: list[dict[str, Any]],
        token_state: dict[str, dict[str, Any]],
    ) -> None:
        for record in base_records:
            if record.blockchain != Blockchain.SOLANA.value or record.token_contract or not record.value:
                continue

            direction = cls._record_direction(wallet_address, record)
            if direction not in {"send", "receive", "trade", "deposit", "withdraw"}:
                continue

            candidate_mints: set[str] = set()
            candidate_decimals: dict[str, Optional[int]] = {}
            for transfer in parsed_transfers:
                source_state = token_state.get(transfer["source"]) or {}
                destination_state = token_state.get(transfer["destination"]) or {}
                mint = str(source_state.get("mint") or destination_state.get("mint") or "")
                if not mint or str(transfer.get("amount") or "") != str(record.value):
                    continue

                transfer_direction = cls._parsed_transfer_direction(
                    wallet_address,
                    transfer,
                    destination_state.get("owner"),
                )
                normalized_direction = "send" if direction in {"trade", "deposit", "withdraw"} else direction
                if transfer_direction != normalized_direction:
                    continue

                decimals = source_state.get("decimals")
                if decimals is None:
                    decimals = destination_state.get("decimals")
                candidate_mints.add(mint)
                candidate_decimals[mint] = decimals

            if len(candidate_mints) != 1:
                continue

            mint = next(iter(candidate_mints))
            record.token_contract = mint
            if record.token_decimals is None:
                record.token_decimals = candidate_decimals.get(mint)
            if mint == cls.WRAPPED_SOL_MINT and (record.token_symbol == "SOL" or record.currency == "SOL"):
                record.token_name = "Wrapped SOL"
                record.token_symbol = "WSOL"
                record.currency = "WSOL"

    @staticmethod
    def _default_solana_rules_path() -> Optional[Path]:
        env_path = Path.home() / "workspace" / "accountsv2" / "sources" / "richard" / "crypto" / "wallet" / "solana" / "_rules.json"
        return env_path if env_path.exists() else None

    def _load_solana_mint_labels(self) -> dict[str, str]:
        if self._solana_mint_labels is not None:
            return self._solana_mint_labels

        path = self._default_solana_rules_path()
        if path is None:
            self._solana_mint_labels = {}
            return self._solana_mint_labels

        try:
            payload = json.loads(path.read_text())
        except (OSError, json.JSONDecodeError):
            self._solana_mint_labels = {}
            return self._solana_mint_labels

        rules = payload.get("rules") if isinstance(payload, dict) else []
        labels: dict[str, str] = {}
        for rule in rules if isinstance(rules, list) else []:
            if not isinstance(rule, dict):
                continue
            commodity = str(rule.get("commodity") or "").strip()
            pattern = str(rule.get("pattern") or "").strip()
            if not commodity or not pattern.startswith("*") or not pattern.endswith("*"):
                continue
            mint = pattern.strip("*").strip()
            if mint:
                labels[mint] = commodity

        self._solana_mint_labels = labels
        return labels

    def _solana_label_for_mint(self, mint: str) -> Optional[tuple[str, str]]:
        if not mint:
            return None
        if mint == self.WRAPPED_SOL_MINT:
            return ("Wrapped SOL", "WSOL")

        commodity = self._load_solana_mint_labels().get(mint)
        if not commodity:
            return None
        if commodity == "Wrapped_SOL":
            return ("Wrapped SOL", "WSOL")
        return (commodity, commodity)

    def _fetch_parsed_solana_transaction(self, tx_hash: str) -> Optional[dict[str, Any]]:
        if self.solana_rpc_client is None:
            return None
        try:
            return self.solana_rpc_client._rpc_call(
                "getTransaction",
                [
                    tx_hash,
                    {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 0},
                ],
            )
        except Exception:
            logger.warning("Failed to fetch parsed Solana transaction %s for Zerion enrichment", tx_hash)
            return None

    @classmethod
    def _set_token_account_owner_if_missing(
        cls,
        token_state: dict[str, dict[str, Any]],
        token_account: Optional[str],
        owner: Optional[str],
    ) -> None:
        if token_account in (None, "") or owner in (None, ""):
            return
        state = token_state.setdefault(
            str(token_account),
            {
                "mint": None,
                "decimals": None,
                "owner": None,
                "pre": 0,
                "post": 0,
                "delta": 0,
            },
        )
        if not state.get("owner"):
            state["owner"] = str(owner)

    @classmethod
    def _apply_wallet_authority_hints_to_token_state(
        cls,
        wallet_address: str,
        token_state: dict[str, dict[str, Any]],
        token_instructions: list[dict[str, Any]],
    ) -> None:
        for instruction in token_instructions:
            if instruction.get("authority") != wallet_address:
                continue
            if instruction.get("type") in {"transfer", "transferChecked"}:
                cls._set_token_account_owner_if_missing(token_state, instruction.get("source"), wallet_address)
            elif instruction.get("type") in {"burn", "burnChecked"}:
                cls._set_token_account_owner_if_missing(token_state, instruction.get("account"), wallet_address)

    def _build_solana_candidate_record(
        self,
        *,
        record_id: str,
        tx_hash: str,
        timestamp: int,
        from_address: str,
        to_address: str,
        value: str,
        fee: str,
        status: str,
        block_number: Any,
        method: str,
        mint: str,
        decimals: Optional[int],
        method_id: Optional[str],
        function_name: Optional[str],
    ) -> TransactionRecord:
        record = TransactionRecord(
            record_id=record_id,
            provider="zerion",
            network=Blockchain.SOLANA.value,
            tx_hash=tx_hash,
            blockchain=Blockchain.SOLANA.value,
            timestamp=timestamp,
            from_address=from_address,
            to_address=to_address,
            value=value,
            fee=fee,
            status=status,
            tx_type="token_transfer",
            token_contract=mint,
            token_decimals=decimals,
            block_number=block_number,
            method=method,
            currency=mint,
            method_id=method_id,
            function_name=function_name,
        )
        label = self._solana_label_for_mint(mint)
        if label:
            record.token_name = label[0]
            record.token_symbol = label[1]
            record.currency = label[1]
        return record

    def _append_solana_burn_companion_receives(
        self,
        wallet: WalletConfig,
        item_id: str,
        tx_hash: str,
        timestamp: int,
        fee: str,
        status: str,
        block_number: Any,
        method_id: Optional[str],
        function_name: Optional[str],
        token_state: dict[str, dict[str, Any]],
        token_instructions: list[dict[str, Any]],
        existing_fingerprints: set[tuple[str, str, str]],
        candidate_records: list[TransactionRecord],
    ) -> None:
        by_outer_index: dict[Any, list[dict[str, Any]]] = {}
        for instruction in token_instructions:
            by_outer_index.setdefault(instruction.get("outer_index"), []).append(instruction)

        for outer_index, instructions in by_outer_index.items():
            burns = [
                instruction
                for instruction in instructions
                if instruction.get("type") in {"burn", "burnChecked"}
                and instruction.get("authority") == wallet.address
            ]
            if not burns:
                continue

            placeholder_accounts = {str(burn.get("account") or "") for burn in burns if burn.get("account")}
            transfers = [
                instruction
                for instruction in instructions
                if instruction.get("type") in {"transfer", "transferChecked"}
                and instruction.get("authority") != wallet.address
            ]
            for transfer in transfers:
                if transfer.get("destination") in placeholder_accounts or transfer.get("source") in placeholder_accounts:
                    continue

                source_state = token_state.get(str(transfer.get("source") or "")) or {}
                destination_state = token_state.get(str(transfer.get("destination") or "")) or {}
                mint = str(source_state.get("mint") or destination_state.get("mint") or "")
                if not mint:
                    continue
                decimals = source_state.get("decimals")
                if decimals is None:
                    decimals = destination_state.get("decimals")
                value = str(transfer.get("amount") or "")
                if not value:
                    continue
                fingerprint = (mint, value, "receive")
                if (
                    fingerprint in existing_fingerprints
                    or self._is_wsol_duplicate_of_base_sol(mint, "receive", existing_fingerprints)
                ):
                    continue
                candidate_records.append(
                    self._build_solana_candidate_record(
                        record_id=f"zerion:{item_id}:rpc:{outer_index}:burn-companion:receive:{mint}",
                        tx_hash=tx_hash,
                        timestamp=timestamp,
                        from_address=str(source_state.get("owner") or transfer.get("source") or ""),
                        to_address=wallet.address,
                        value=value,
                        fee=fee,
                        status=status,
                        block_number=block_number,
                        method="receive",
                        mint=mint,
                        decimals=decimals,
                        method_id=method_id,
                        function_name=function_name,
                    )
                )
                existing_fingerprints.add(fingerprint)

    def _append_wallet_mint_receives(
        self,
        wallet: WalletConfig,
        item_id: str,
        tx_hash: str,
        timestamp: int,
        fee: str,
        status: str,
        block_number: Any,
        method_id: Optional[str],
        function_name: Optional[str],
        token_state: dict[str, dict[str, Any]],
        token_instructions: list[dict[str, Any]],
        existing_fingerprints: set[tuple[str, str, str]],
        candidate_records: list[TransactionRecord],
        response_has_owner_data: bool,
    ) -> None:
        for instruction in token_instructions:
            if instruction.get("type") not in {"mintTo", "mintToChecked"}:
                continue

            token_account = str(instruction.get("account") or "")
            account_state = token_state.get(token_account) or {}
            # Mirror the transfer-side fallback: pre-2022 RPC responses
            # don't carry `owner` on token balances, so when no row in the
            # response has owner data, fall back to the outer-accounts
            # heuristic. Without this, every Marinade-era stake-mint into
            # the wallet's ATA gets dropped.
            if account_state.get("owner") != wallet.address and (
                response_has_owner_data
                or wallet.address not in instruction.get("outer_accounts", [])
            ):
                continue

            mint = str(instruction.get("mint") or account_state.get("mint") or "")
            if not mint:
                continue
            value = str(instruction.get("amount") or "")
            # Wormhole-style bridges emit a sentinel `mintTo amount=0` alongside
            # the real one — never emit those as wallet receives.
            if not value or value == "0":
                continue
            # Zerion's base record for the same on-chain mintTo will report the
            # inbound flow with method "deposit" / "mint" / "trade" depending on
            # the operation type. `_existing_record_fingerprints` adds a
            # "receive" alias for any wallet-inbound record so this single check
            # covers all those direction-word variants.
            fingerprint = (mint, value, "receive")
            if fingerprint in existing_fingerprints:
                continue

            # Two mintTo at the same outer instruction with the same mint
            # (e.g. an init followed by the real mint) would otherwise share
            # an id; disambiguate with the inner-instruction index.
            inner_index = instruction.get("inner_index")
            inner_suffix = f":{inner_index}" if inner_index is not None else ""
            candidate_records.append(
                self._build_solana_candidate_record(
                    record_id=f"zerion:{item_id}:rpc:{instruction['outer_index']}{inner_suffix}:mint-receive:{mint}",
                    tx_hash=tx_hash,
                    timestamp=timestamp,
                    from_address=str(
                        instruction.get("mint_authority")
                        or instruction.get("outer_program_id")
                        or token_account
                    ),
                    to_address=wallet.address,
                    value=value,
                    fee=fee,
                    status=status,
                    block_number=block_number,
                    method="receive",
                    mint=mint,
                    decimals=account_state.get("decimals"),
                    method_id=method_id,
                    function_name=function_name,
                )
            )
            existing_fingerprints.add(fingerprint)

    def _etherscan_client_for_network(self, network: Optional[str]) -> Optional[EtherscanClient]:
        if not self.etherscan_api_key:
            return None
        chain_id = EtherscanClient.chain_id_for_network(network or Blockchain.ETHEREUM.value)
        if chain_id is None:
            return None
        client = self._etherscan_clients.get(chain_id)
        if client is None:
            client = EtherscanClient(api_key=self.etherscan_api_key, chain_id=chain_id)
            self._etherscan_clients[chain_id] = client
        return client

    @staticmethod
    def _apply_ethereum_metadata(record: TransactionRecord, metadata: dict[str, Any]) -> None:
        if record.block_number is None and metadata.get("block_number") is not None:
            record.block_number = int(metadata["block_number"])
        if record.gas_used in (None, "") and metadata.get("gas_used") not in (None, ""):
            record.gas_used = str(metadata["gas_used"])
        if record.gas_price in (None, "") and metadata.get("gas_price") not in (None, ""):
            record.gas_price = str(metadata["gas_price"])
        if record.input_data in (None, "") and metadata.get("input_data") not in (None, ""):
            record.input_data = str(metadata["input_data"])
        if record.tx_receipt_status in (None, "") and metadata.get("tx_receipt_status") not in (None, ""):
            record.tx_receipt_status = str(metadata["tx_receipt_status"])
        if record.transaction_index is None and metadata.get("transaction_index") is not None:
            record.transaction_index = int(metadata["transaction_index"])
        if record.cumulative_gas_used in (None, "") and metadata.get("cumulative_gas_used") not in (None, ""):
            record.cumulative_gas_used = str(metadata["cumulative_gas_used"])
        if record.confirmations is None and metadata.get("confirmations") is not None:
            record.confirmations = int(metadata["confirmations"])
        if record.method_id in (None, "") and metadata.get("method_id") not in (None, ""):
            record.method_id = str(metadata["method_id"])
        if record.fee in (None, "", "0") and record.gas_used not in (None, "") and record.gas_price not in (None, ""):
            record.fee = str(int(record.gas_used) * int(record.gas_price))
        if record.status in ("", "unknown") and record.tx_receipt_status in {"0", "1"}:
            record.status = "success" if record.tx_receipt_status == "1" else "failed"

    def _enrich_ethereum_records(self, records: list[TransactionRecord]) -> list[TransactionRecord]:
        grouped_records: dict[tuple[str, str], list[TransactionRecord]] = {}
        for record in records:
            if record.blockchain != Blockchain.ETHEREUM.value or not record.tx_hash:
                continue
            network = str(record.network or Blockchain.ETHEREUM.value)
            grouped_records.setdefault((network, record.tx_hash), []).append(record)

        for (network, tx_hash), grouped in grouped_records.items():
            client = self._etherscan_client_for_network(network)
            if client is None:
                continue
            proxy_source = etherscan_proxy_source(
                self.cache,
                client.chain_id,
                self.cache_mode,
                lambda action, params: client._proxy_request(action, **params),
            )
            try:
                if proxy_source is None:
                    metadata = client.fetch_transaction_metadata(tx_hash)
                else:
                    metadata = client.fetch_transaction_metadata(tx_hash, proxy_source=proxy_source)
            except Exception:
                logger.warning(
                    "Failed to fetch Ethereum metadata for Zerion tx %s on %s",
                    tx_hash,
                    network,
                )
                continue
            if not metadata:
                continue
            for record in grouped:
                self._apply_ethereum_metadata(record, metadata)

        return records

    def _enrich_sparse_solana_records(
        self,
        wallet: WalletConfig,
        item: dict[str, Any],
        base_records: list[TransactionRecord],
    ) -> list[TransactionRecord]:
        if wallet.blockchain != Blockchain.SOLANA or not base_records:
            return base_records

        tx_hash = base_records[0].tx_hash
        if not tx_hash:
            return base_records

        tx_data = self._fetch_parsed_solana_transaction(tx_hash)
        if not tx_data or not isinstance(tx_data, dict):
            return base_records

        meta = tx_data.get("meta") or {}
        if meta.get("err") is not None:
            return base_records

        # closeAccount on a wallet-owned wSOL ATA returns the ATA's lamports
        # (rent + any wSOL balance) to the wallet. The underlying SOL flow
        # was already credited at the swap that funded the ATA — a separate
        # earlier tx — so emitting the close as a fresh receive double-
        # credits the wallet's leaf balance.
        base_records = self._drop_self_owned_wsol_ata_close_receives(
            base_records, tx_data, wallet.address,
        )
        if not base_records:
            return base_records

        token_state = self._build_token_account_state(tx_data)
        if not token_state:
            return base_records

        # Pre-2022 Helius getTransaction responses don't populate `owner`
        # on `pre/postTokenBalances`. When the field is absent across the
        # whole response we can't enforce the strict owner-equality guard
        # below, so we fall back to the outer-accounts heuristic the
        # pipeline used before that field existed.
        response_has_owner_data = any(
            state.get("owner") for state in token_state.values()
        )

        item_id = str(item.get("id") or tx_hash)
        attributes = item.get("attributes") or {}
        timestamp = self._parse_timestamp(attributes.get("mined_at")) or int(tx_data.get("blockTime") or 0)
        block_number = attributes.get("mined_at_block") or tx_data.get("slot")
        fee = self._extract_fee(attributes) or str(meta.get("fee") or "0")
        method_id, function_name = self._extract_method_metadata(attributes)
        account_keys = self._extract_account_keys(tx_data)
        outer_instructions = ((tx_data.get("transaction") or {}).get("message") or {}).get("instructions") or []

        token_instructions: list[dict[str, Any]] = []
        parsed_transfers: list[dict[str, Any]] = []

        def collect(instruction: dict[str, Any], outer_index: Any, inner_index: Any,
                    outer_accounts: list[str], outer_program_id: Optional[str]) -> None:
            if instruction.get("program") != "spl-token":
                return
            parsed = instruction.get("parsed") or {}
            instruction_type = parsed.get("type")
            if instruction_type not in {
                "transfer",
                "transferChecked",
                "burn",
                "burnChecked",
                "mintTo",
                "mintToChecked",
            }:
                return
            info = parsed.get("info") or {}
            amount = self._extract_spl_token_amount(info)
            token_instruction = {
                "outer_index": outer_index,
                "inner_index": inner_index,
                "outer_accounts": outer_accounts,
                "outer_program_id": outer_program_id,
                "type": str(instruction_type),
                "amount": amount,
                "source": str(info.get("source")) if info.get("source") not in (None, "") else None,
                "destination": str(info.get("destination")) if info.get("destination") not in (None, "") else None,
                "authority": str(info.get("authority")) if info.get("authority") not in (None, "") else None,
                "account": str(info.get("account")) if info.get("account") not in (None, "") else None,
                "mint": str(info.get("mint")) if info.get("mint") not in (None, "") else None,
                "mint_authority": str(info.get("mintAuthority")) if info.get("mintAuthority") not in (None, "") else None,
            }
            token_instructions.append(token_instruction)
            if instruction_type not in {"transfer", "transferChecked"}:
                return
            if amount in (None, "") or token_instruction["source"] in (None, "") or token_instruction["destination"] in (None, ""):
                return
            parsed_transfers.append(token_instruction)

        # Inner instructions: spl-token CPIs emitted by a wrapper program
        # (Serum, Raydium, Jupiter, etc.).
        for inner_group in meta.get("innerInstructions") or []:
            outer_index = inner_group.get("index")
            outer_instruction = (
                outer_instructions[outer_index]
                if isinstance(outer_index, int) and 0 <= outer_index < len(outer_instructions)
                else {}
            )
            outer_accounts = self._resolve_instruction_accounts(outer_instruction, account_keys)
            outer_program_id = self._extract_outer_program_id(outer_instruction, account_keys)
            for inner_index, instruction in enumerate(inner_group.get("instructions") or []):
                collect(instruction, outer_index, inner_index, outer_accounts, outer_program_id)

        # Outer instructions: direct spl-token calls signed by the wallet
        # itself (e.g. ATA→ATA transfers between user-owned wallets).
        # These produce no innerInstructions, so the loop above misses
        # them entirely.
        for outer_index, outer_instruction in enumerate(outer_instructions):
            if outer_instruction.get("program") != "spl-token":
                continue
            outer_accounts = self._resolve_instruction_accounts(outer_instruction, account_keys)
            outer_program_id = self._extract_outer_program_id(outer_instruction, account_keys)
            collect(outer_instruction, outer_index, None, outer_accounts, outer_program_id)

        if not parsed_transfers and not token_instructions:
            return base_records

        self._apply_wallet_authority_hints_to_token_state(wallet.address, token_state, token_instructions)
        self._upgrade_solana_base_records(wallet.address, base_records, parsed_transfers, token_state)
        existing_fingerprints = self._existing_record_fingerprints(wallet.address, base_records)

        send_mints: set[str] = set()
        candidate_records: list[TransactionRecord] = []
        candidate_receive_accounts: dict[tuple[Any, str], dict[str, Any]] = {}

        for transfer in parsed_transfers:
            source_state = token_state.get(transfer["source"]) or {}
            destination_state = token_state.get(transfer["destination"]) or {}
            mint = str(source_state.get("mint") or destination_state.get("mint") or "")
            if not mint:
                continue
            decimals = source_state.get("decimals")
            if decimals is None:
                decimals = destination_state.get("decimals")

            if transfer.get("authority") == wallet.address:
                send_mints.add(mint)
                fingerprint = (mint, transfer["amount"], "send")
                if fingerprint not in existing_fingerprints and not self._is_wsol_duplicate_of_base_sol(
                    mint, "send", existing_fingerprints,
                ):
                    candidate_records.append(
                        self._build_solana_candidate_record(
                            record_id=f"zerion:{item_id}:rpc:{transfer['outer_index']}:send:{mint}",
                            tx_hash=tx_hash,
                            timestamp=timestamp,
                            from_address=wallet.address,
                            to_address=transfer["destination"],
                            value=transfer["amount"],
                            fee=fee,
                            status=self._normalize_status(attributes.get("status")),
                            block_number=block_number,
                            method="send",
                            mint=mint,
                            decimals=decimals,
                            method_id=method_id,
                            function_name=function_name,
                        )
                    )
                    existing_fingerprints.add(fingerprint)

            destination_delta = int(destination_state.get("delta") or 0)
            if destination_delta <= 0:
                continue
            # When the response carries owner data, enforce strict equality:
            # protocol-managed token accounts can show up in outer-instruction
            # accounts while being owned by an aggregator (Jupiter et al.),
            # so only owner-matched ATAs count as wallet receives.
            # When the response has no owner data at all (legacy txs), fall
            # back to requiring the wallet to be referenced in the outer
            # instruction's account list — that's the only ownership signal
            # available and matches the pre-rewrite emission behaviour.
            if destination_state.get("owner") != wallet.address and (
                response_has_owner_data
                or wallet.address not in transfer["outer_accounts"]
            ):
                continue
            if mint in send_mints:
                continue
            candidate_receive_accounts[(transfer["outer_index"], transfer["destination"])] = {
                "mint": mint,
                "decimals": decimals,
                "source": transfer["source"],
                "outer_program_id": transfer["outer_program_id"],
            }

        is_trade = bool(candidate_records) and bool(candidate_receive_accounts) and any(
            info.get("outer_program_id") not in self.SOLANA_STANDARD_PROGRAM_IDS
            for info in candidate_receive_accounts.values()
            if info.get("outer_program_id")
        )

        for (outer_index, destination), info in sorted(candidate_receive_accounts.items()):
            destination_state = token_state.get(destination) or {}
            value = str(destination_state.get("delta") or "0")
            fingerprint = (info["mint"], value, "receive")
            if (
                fingerprint in existing_fingerprints
                or self._is_wsol_duplicate_of_base_sol(info["mint"], "receive", existing_fingerprints)
            ):
                continue
            candidate_records.append(
                self._build_solana_candidate_record(
                    record_id=f"zerion:{item_id}:rpc:{outer_index}:receive:{info['mint']}",
                    tx_hash=tx_hash,
                    timestamp=timestamp,
                    from_address=str(info.get("source") or ""),
                    to_address=wallet.address,
                    value=value,
                    fee=fee,
                    status=self._normalize_status(attributes.get("status")),
                    block_number=block_number,
                    method="trade" if is_trade else "receive",
                    mint=info["mint"],
                    decimals=info.get("decimals"),
                    method_id=method_id,
                    function_name=function_name,
                )
            )
            existing_fingerprints.add(fingerprint)

        self._append_wallet_mint_receives(
            wallet,
            item_id,
            tx_hash,
            timestamp,
            fee,
            self._normalize_status(attributes.get("status")),
            block_number,
            method_id,
            function_name,
            token_state,
            token_instructions,
            existing_fingerprints,
            candidate_records,
            response_has_owner_data,
        )
        self._append_solana_burn_companion_receives(
            wallet,
            item_id,
            tx_hash,
            timestamp,
            fee,
            self._normalize_status(attributes.get("status")),
            block_number,
            method_id,
            function_name,
            token_state,
            token_instructions,
            existing_fingerprints,
            candidate_records,
        )

        if is_trade:
            for record in candidate_records:
                if record.method in {"send", "receive"}:
                    record.method = "trade"

        return base_records + candidate_records

    def _normalize_transfer_record(
        self,
        wallet: WalletConfig,
        item: dict[str, Any],
        transfer: dict[str, Any],
        transfer_index: int,
    ) -> TransactionRecord:
        item_id = str(item.get("id") or "")
        attributes = item.get("attributes") or {}
        fungible_info = transfer.get("fungible_info") or {}
        nft_info = transfer.get("nft_info") or {}
        implementations = fungible_info.get("implementations") or []
        item_chain_id = self._extract_chain_id(item) or wallet.network
        implementation = self._select_implementation(implementations, item_chain_id or wallet.network)

        raw_value, numeric, decimals = self._extract_quantity_components(transfer)
        token_decimals = decimals
        if token_decimals is None:
            try:
                token_decimals = int(implementation.get("decimals"))
            except (TypeError, ValueError):
                token_decimals = None

        tx_hash = str(attributes.get("hash") or item_id)
        # Item-level chain wins: a single tx hash exists on exactly one chain,
        # but Zerion sometimes stamps individual transfers with a contradictory
        # chain_id (e.g. native ETH leg of an Arbitrum swap labeled "abstract").
        chain_id = item_chain_id or self._extract_chain_id(item, transfer=transfer) or wallet.network
        token_symbol = fungible_info.get("symbol") or nft_info.get("collection_name")
        token_name = fungible_info.get("name") or nft_info.get("name")
        token_contract = implementation.get("address") or transfer.get("contract_address")
        method_id, function_name = self._extract_method_metadata(attributes)

        return TransactionRecord(
            record_id=f"zerion:{item_id}:{transfer_index}",
            provider="zerion",
            network=chain_id,
            tx_hash=tx_hash,
            blockchain=self._blockchain_for(wallet, chain_id),
            timestamp=self._parse_timestamp(attributes.get("mined_at")),
            from_address=self._first_present(
                transfer.get("from"),
                transfer.get("sender"),
                transfer.get("sent_from"),
                transfer.get("from_address"),
                attributes.get("sent_from"),
            ) or "",
            to_address=self._first_present(
                transfer.get("to"),
                transfer.get("recipient"),
                transfer.get("received_to"),
                transfer.get("to_address"),
                attributes.get("sent_to"),
            ) or "",
            value=self._to_base_units(raw_value, numeric, token_decimals),
            fee=self._extract_fee(attributes),
            status=self._normalize_status(attributes.get("status")),
            tx_type="nft_transfer" if nft_info else "token_transfer",
            token_name=str(token_name) if token_name not in (None, "") else None,
            token_symbol=str(token_symbol) if token_symbol not in (None, "") else None,
            token_contract=str(token_contract) if token_contract not in (None, "") else None,
            token_decimals=token_decimals,
            block_number=attributes.get("mined_at_block"),
            method=str(attributes.get("operation_type")) if attributes.get("operation_type") else None,
            currency=str(token_symbol) if token_symbol not in (None, "") else None,
            method_id=method_id,
            function_name=function_name,
        )

    def _normalize_transaction(
        self,
        wallet: WalletConfig,
        item: dict[str, Any],
    ) -> list[TransactionRecord]:
        item_id = str(item.get("id") or "")
        attributes = item.get("attributes") or {}
        transfers = attributes.get("transfers") or []
        method_id, function_name = self._extract_method_metadata(attributes)
        if isinstance(transfers, list) and transfers:
            records = [
                self._normalize_transfer_record(wallet, item, transfer, index)
                for index, transfer in enumerate(transfers)
            ]
            records = self._enrich_sparse_solana_records(wallet, item, records)
            return self._enrich_ethereum_records(records)

        tx_hash = str(attributes.get("hash") or item_id)
        chain_id = self._extract_chain_id(item) or wallet.network
        records = [
            TransactionRecord(
                record_id=f"zerion:{item_id}",
                provider="zerion",
                network=chain_id,
                tx_hash=tx_hash,
                blockchain=self._blockchain_for(wallet, chain_id),
                timestamp=self._parse_timestamp(attributes.get("mined_at")),
                from_address=self._first_present(attributes.get("sent_from")) or "",
                to_address=self._first_present(attributes.get("sent_to")) or "",
                value="0",
                fee=self._extract_fee(attributes),
                status=self._normalize_status(attributes.get("status")),
                tx_type=str(attributes.get("operation_type")) if attributes.get("operation_type") else "transaction",
                block_number=attributes.get("mined_at_block"),
                method=str(attributes.get("operation_type")) if attributes.get("operation_type") else None,
                method_id=method_id,
                function_name=function_name,
            )
        ]
        records = self._enrich_sparse_solana_records(wallet, item, records)
        return self._enrich_ethereum_records(records)

    @staticmethod
    def _filter_records_against_state(
        page_records: list[TransactionRecord],
        latest_timestamp: int,
        latest_record_ids: set[str],
    ) -> tuple[list[TransactionRecord], bool]:
        if not latest_timestamp:
            return page_records, False

        filtered_records: list[TransactionRecord] = []
        boundary_hit = False
        for record in page_records:
            if record.timestamp > latest_timestamp:
                filtered_records.append(record)
                continue
            if record.timestamp == latest_timestamp and record.record_id not in latest_record_ids:
                filtered_records.append(record)
                continue
            boundary_hit = True
        return filtered_records, boundary_hit

    @classmethod
    def advance_state(
        cls,
        previous_state: Optional[dict[str, Any]],
        records: list[TransactionRecord],
    ) -> dict[str, Any]:
        next_state = dict(previous_state or {})
        next_state["provider"] = "zerion"
        if not records:
            return next_state

        current_latest_timestamp = int(next_state.get("latest_timestamp") or 0)
        current_latest_record_ids = set(next_state.get("latest_record_ids") or [])
        latest_timestamp = max(record.timestamp for record in records)
        latest_record_ids = {
            record.record_id
            for record in records
            if record.timestamp == latest_timestamp and record.record_id
        }

        if latest_timestamp > current_latest_timestamp:
            next_state["latest_timestamp"] = latest_timestamp
            next_state["latest_record_ids"] = sorted(latest_record_ids)
            return next_state

        if latest_timestamp == current_latest_timestamp and latest_record_ids:
            next_state["latest_timestamp"] = latest_timestamp
            next_state["latest_record_ids"] = sorted(current_latest_record_ids | latest_record_ids)

        return next_state

    @classmethod
    def _next_state(cls, records: list[TransactionRecord], previous_state: dict[str, Any]) -> dict[str, Any]:
        return cls.advance_state(previous_state, records)

    def _iter_api_pages(self, wallet: WalletConfig) -> Iterator[dict[str, Any]]:
        """Yield raw API page payloads, following ``links.next`` until exhausted.

        Pure I/O — does not normalize or filter.  The cache layer can supply an
        equivalent generator that reads payloads from disk instead.
        """
        request_url = f"{self.BASE_URL}/wallets/{wallet.address}/transactions/"
        request_params = self._initial_params(wallet)

        while True:
            payload = self._request(request_url, params=request_params)
            yield payload
            next_url = (payload.get("links") or {}).get("next")
            if not next_url:
                break
            request_url, request_params = self._params_from_next_url(str(next_url))

    def fetch_new_transaction_batches(
        self,
        wallet: WalletConfig,
        state: Optional[dict[str, Any]] = None,
        page_source: Optional[Iterator[dict[str, Any]]] = None,
    ) -> Iterator[list[TransactionRecord]]:
        state = state or {}
        latest_timestamp = int(state.get("latest_timestamp") or 0)
        latest_record_ids = set(state.get("latest_record_ids") or [])

        pages = page_source if page_source is not None else self._iter_api_pages(wallet)
        for payload in pages:
            page_records: list[TransactionRecord] = []
            for item in payload.get("data", []):
                page_records.extend(self._normalize_transaction(wallet, item))

            page_records, boundary_hit = self._filter_records_against_state(
                page_records,
                latest_timestamp,
                latest_record_ids,
            )
            if page_records:
                yield page_records

            if boundary_hit:
                break

    def fetch_new_transactions(
        self,
        wallet: WalletConfig,
        state: Optional[dict[str, Any]] = None,
    ) -> tuple[list[TransactionRecord], dict[str, Any]]:
        state = state or {}
        all_records: list[TransactionRecord] = []
        next_state = self.advance_state(state, [])

        for page_records in self.fetch_new_transaction_batches(wallet, state=state):
            all_records.extend(page_records)
            next_state = self.advance_state(next_state, page_records)

        return all_records, next_state
