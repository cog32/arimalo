"""Binance account activity sync and CSV export."""
from __future__ import annotations

import csv
import hashlib
import hmac
import json
import logging
import re
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable, Optional
from urllib.parse import urlencode

import requests

from sync_crypto.clients import RateLimiter
from sync_crypto.models import BinanceAccountingEvent, BinanceConfig

logger = logging.getLogger(__name__)

BINANCE_EVENT_FIELDS = [
    "event_id",
    "exchange",
    "account_name",
    "timestamp",
    "event_type",
    "status",
    "asset",
    "asset_amount",
    "counter_asset",
    "counter_amount",
    "fee_asset",
    "fee_amount",
    "symbol",
    "side",
    "source_account",
    "destination_account",
    "network",
    "address",
    "address_tag",
    "tx_id",
    "order_id",
    "trade_id",
    "quote_id",
    "transfer_id",
    "reference",
    "metadata_json",
]

BINANCE_HISTORY_START_MS = 1501545600000
DUST_HISTORY_START_MS = 1606780800000
MS_PER_DAY = 24 * 60 * 60 * 1000
DEPOSIT_WITHDRAW_WINDOW_MS = 90 * MS_PER_DAY
CONVERT_WINDOW_MS = 30 * MS_PER_DAY
DIVIDEND_WINDOW_MS = 180 * MS_PER_DAY
SIMPLE_EARN_WINDOW_MS = 30 * MS_PER_DAY
UNIVERSAL_TRANSFER_RETENTION_MS = 180 * MS_PER_DAY


class BinanceRowLimitReached(Exception):
    """Raised when a Binance sync hits the requested row limit."""


def _safe_name(value: str) -> str:
    return re.sub(r"[^A-Za-z0-9_.-]+", "_", value.strip()) or "binance"


def binance_csv_path(config: BinanceConfig, output_dir: Path) -> Path:
    """Return the Binance CSV path for an account config."""
    output_dir.mkdir(parents=True, exist_ok=True)
    return output_dir / f"binance_{_safe_name(config.friendly_name)}_accounting.csv"


def binance_state_path(config: BinanceConfig, output_dir: Path) -> Path:
    """Return the Binance sync state path for an account config."""
    output_dir.mkdir(parents=True, exist_ok=True)
    return output_dir / f"binance_{_safe_name(config.friendly_name)}_state.json"


class BinanceClient:
    """Client for Binance signed account endpoints."""

    BASE_URL = "https://api.binance.com"
    MAX_RETRIES = 5

    def __init__(
        self,
        api_key: str,
        api_secret: str,
        base_url: Optional[str] = None,
    ):
        self.api_key = api_key
        self.api_secret = api_secret.encode("utf-8")
        self.base_url = (base_url or self.BASE_URL).rstrip("/")
        self.rate_limiter = RateLimiter(calls_per_second=5)

    def _request_public(self, path: str, params: Optional[dict[str, Any]] = None) -> Any:
        response = None
        for attempt in range(self.MAX_RETRIES):
            self.rate_limiter.wait()
            response = requests.get(
                f"{self.base_url}{path}",
                params=params or {},
                timeout=30,
            )
            if response.status_code == 429:
                wait = 2 ** attempt + 5
                logger.warning(
                    "Binance rate limited on %s (429), backing off %ds...",
                    path,
                    wait,
                )
                time.sleep(wait)
                self.rate_limiter.last_call = time.monotonic()
                continue
            response.raise_for_status()
            return response.json()
        assert response is not None
        response.raise_for_status()
        return response.json()

    def _request_signed(self, path: str, params: Optional[dict[str, Any]] = None) -> Any:
        response = None
        for attempt in range(self.MAX_RETRIES):
            payload = {k: v for k, v in (params or {}).items() if v is not None}
            payload["timestamp"] = int(time.time() * 1000)
            query = urlencode(payload, doseq=True)
            signature = hmac.new(
                self.api_secret,
                query.encode("utf-8"),
                hashlib.sha256,
            ).hexdigest()
            signed_payload = dict(payload)
            signed_payload["signature"] = signature

            self.rate_limiter.wait()
            response = requests.get(
                f"{self.base_url}{path}",
                params=signed_payload,
                headers={"X-MBX-APIKEY": self.api_key},
                timeout=30,
            )
            if response.status_code == 429:
                wait = 2 ** attempt + 5
                logger.warning(
                    "Binance rate limited on %s (429), backing off %ds...",
                    path,
                    wait,
                )
                time.sleep(wait)
                self.rate_limiter.last_call = time.monotonic()
                continue
            response.raise_for_status()
            return response.json()
        assert response is not None
        response.raise_for_status()
        return response.json()

    def get_exchange_info(self) -> dict[str, Any]:
        """Fetch current exchange symbol metadata."""
        return self._request_public("/api/v3/exchangeInfo")

    def get_my_trades(
        self,
        symbol: str,
        *,
        from_id: Optional[int] = None,
        order_id: Optional[int] = None,
        limit: int = 1000,
    ) -> list[dict[str, Any]]:
        """Fetch account trade history for one symbol."""
        return self._request_signed(
            "/api/v3/myTrades",
            {
                "symbol": symbol,
                "fromId": from_id,
                "orderId": order_id,
                "limit": limit,
            },
        )

    def get_deposit_history(
        self,
        start_time: int,
        end_time: int,
        *,
        offset: int = 0,
        limit: int = 1000,
    ) -> list[dict[str, Any]]:
        """Fetch deposit history for a time window."""
        return self._request_signed(
            "/sapi/v1/capital/deposit/hisrec",
            {
                "startTime": start_time,
                "endTime": end_time,
                "offset": offset,
                "limit": limit,
            },
        )

    def get_withdraw_history(
        self,
        start_time: int,
        end_time: int,
        *,
        offset: int = 0,
        limit: int = 1000,
    ) -> list[dict[str, Any]]:
        """Fetch withdrawal history for a time window."""
        return self._request_signed(
            "/sapi/v1/capital/withdraw/history",
            {
                "startTime": start_time,
                "endTime": end_time,
                "offset": offset,
                "limit": limit,
            },
        )

    def get_convert_trade_history(
        self,
        start_time: int,
        end_time: int,
        *,
        limit: int = 100,
    ) -> dict[str, Any]:
        """Fetch convert trade history for a time window."""
        return self._request_signed(
            "/sapi/v1/convert/tradeFlow",
            {
                "startTime": start_time,
                "endTime": end_time,
                "limit": limit,
            },
        )

    def get_dust_log(
        self,
        *,
        start_time: Optional[int] = None,
        end_time: Optional[int] = None,
        account_type: str = "SPOT",
    ) -> dict[str, Any]:
        """Fetch Binance dust conversion history."""
        return self._request_signed(
            "/sapi/v1/asset/dribblet",
            {
                "accountType": account_type,
                "startTime": start_time,
                "endTime": end_time,
            },
        )

    def get_asset_dividend_history(
        self,
        start_time: int,
        end_time: int,
        *,
        current: int = 1,
        limit: int = 500,
    ) -> dict[str, Any]:
        """Fetch asset dividend history for a time window."""
        return self._request_signed(
            "/sapi/v1/asset/assetDividend",
            {
                "startTime": start_time,
                "endTime": end_time,
                "current": current,
                "limit": limit,
            },
        )

    def get_universal_transfer_history(
        self,
        transfer_type: str,
        *,
        start_time: int,
        end_time: int,
        current: int = 1,
        size: int = 100,
    ) -> dict[str, Any]:
        """Fetch universal transfer history for one transfer type."""
        return self._request_signed(
            "/sapi/v1/asset/transfer",
            {
                "type": transfer_type,
                "startTime": start_time,
                "endTime": end_time,
                "current": current,
                "size": size,
            },
        )

    def get_flexible_rewards_history(
        self,
        start_time: int,
        end_time: int,
        *,
        current: int = 1,
        size: int = 100,
        reward_type: str = "ALL",
    ) -> dict[str, Any]:
        """Fetch flexible Simple Earn rewards for a time window."""
        return self._request_signed(
            "/sapi/v1/simple-earn/flexible/history/rewardsRecord",
            {
                "startTime": start_time,
                "endTime": end_time,
                "current": current,
                "size": size,
                "type": reward_type,
            },
        )

    def get_locked_rewards_history(
        self,
        start_time: int,
        end_time: int,
        *,
        current: int = 1,
        size: int = 100,
    ) -> dict[str, Any]:
        """Fetch locked Simple Earn rewards for a time window."""
        return self._request_signed(
            "/sapi/v1/simple-earn/locked/history/rewardsRecord",
            {
                "startTime": start_time,
                "endTime": end_time,
                "current": current,
                "size": size,
            },
        )


def _load_state(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text())
    except Exception:
        logger.warning("Ignoring unreadable Binance sync state at %s", path)
        return {}


def _save_state(path: Path, state: dict[str, Any]) -> None:
    path.write_text(json.dumps(state, indent=2, sort_keys=True))


def _existing_event_ids(path: Path) -> set[str]:
    if not path.exists():
        return set()
    with open(path, newline="") as handle:
        reader = csv.DictReader(handle)
        return {row["event_id"] for row in reader if row.get("event_id")}


def _write_events(
    events: Iterable[BinanceAccountingEvent],
    config: BinanceConfig,
    output_dir: Path,
) -> Path:
    path = binance_csv_path(config, output_dir)
    events = sorted(events, key=lambda event: (event.timestamp, event.event_id))

    if path.exists():
        existing_ids = _existing_event_ids(path)
        new_events = [event for event in events if event.event_id not in existing_ids]
        with open(path, "a", newline="") as handle:
            writer = csv.DictWriter(handle, fieldnames=BINANCE_EVENT_FIELDS)
            for event in new_events:
                writer.writerow(event.to_csv_row())
        written = len(new_events)
    else:
        with open(path, "w", newline="") as handle:
            writer = csv.DictWriter(handle, fieldnames=BINANCE_EVENT_FIELDS)
            writer.writeheader()
            for event in events:
                writer.writerow(event.to_csv_row())
        written = len(events)

    logger.info("Wrote %d Binance accounting events to %s", written, path)
    return path


def _signed_amount(value: Any, sign: int) -> str:
    text = str(value).strip()
    if sign >= 0 or text in {"", "0", "0.0", "0.00", "0.00000000"}:
        return text
    return f"-{text.lstrip('+')}"


def _parse_int(value: Any) -> Optional[int]:
    if value in (None, ""):
        return None
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


def _parse_utc_datetime(value: Any) -> Optional[int]:
    if value in (None, ""):
        return None
    text = str(value).strip()
    for fmt in ("%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S"):
        try:
            dt = datetime.strptime(text, fmt).replace(tzinfo=timezone.utc)
            return int(dt.timestamp() * 1000)
        except ValueError:
            continue
    return _parse_int(value)


def _wallet_type_name(value: Any) -> Optional[str]:
    wallet_type = _parse_int(value)
    if wallet_type == 0:
        return "spot"
    if wallet_type == 1:
        return "funding"
    if wallet_type is None:
        return None
    return str(wallet_type)


def _json_metadata(payload: dict[str, Any]) -> str:
    return json.dumps(payload, sort_keys=True, separators=(",", ":"))


def _incremental_start(last_timestamp_ms: Optional[int], default_start_ms: int) -> int:
    if last_timestamp_ms is None:
        return default_start_ms
    return max(default_start_ms, max(0, last_timestamp_ms - 1000))


def _window_ranges(start_ms: int, end_ms: int, window_ms: int) -> list[tuple[int, int]]:
    windows = []
    cursor = start_ms
    while cursor <= end_ms:
        window_end = min(cursor + window_ms - 1, end_ms)
        windows.append((cursor, window_end))
        cursor = window_end + 1
    return windows


def _store_event(
    events: dict[str, BinanceAccountingEvent],
    existing_event_ids: set[str],
    event: BinanceAccountingEvent,
    max_rows: Optional[int],
) -> tuple[bool, bool]:
    if event.event_id in existing_event_ids or event.event_id in events:
        return False, False
    events[event.event_id] = event
    limit_reached = max_rows is not None and len(events) >= max_rows
    return True, limit_reached


def _normalize_trade(
    config: BinanceConfig,
    symbol_meta: dict[str, dict[str, Any]],
    raw: dict[str, Any],
) -> BinanceAccountingEvent:
    symbol = str(raw.get("symbol") or "")
    meta = symbol_meta.get(symbol, {})
    base_asset = str(meta.get("baseAsset") or symbol)
    quote_asset = str(meta.get("quoteAsset") or "")
    is_buyer = bool(raw.get("isBuyer"))
    side = "buy" if is_buyer else "sell"
    return BinanceAccountingEvent(
        event_id=f"spot_trade:{symbol}:{raw['id']}",
        exchange="binance",
        account_name=config.friendly_name,
        timestamp=int(raw["time"]),
        event_type="spot_trade",
        status="filled",
        asset=base_asset or None,
        asset_amount=_signed_amount(raw.get("qty", ""), 1 if is_buyer else -1),
        counter_asset=quote_asset or None,
        counter_amount=_signed_amount(raw.get("quoteQty", ""), -1 if is_buyer else 1),
        fee_asset=str(raw.get("commissionAsset") or "") or None,
        fee_amount=str(raw.get("commission") or "") or None,
        symbol=symbol or None,
        side=side,
        order_id=str(raw.get("orderId") or "") or None,
        trade_id=str(raw.get("id") or "") or None,
        reference="maker" if raw.get("isMaker") else "taker",
        metadata_json=_json_metadata(
            {
                "isBestMatch": raw.get("isBestMatch"),
                "orderListId": raw.get("orderListId"),
            }
        ),
    )


def _normalize_deposit(config: BinanceConfig, raw: dict[str, Any]) -> BinanceAccountingEvent:
    timestamp = _parse_int(raw.get("completeTime")) or _parse_int(raw.get("insertTime")) or 0
    return BinanceAccountingEvent(
        event_id=f"deposit:{raw['id']}",
        exchange="binance",
        account_name=config.friendly_name,
        timestamp=timestamp,
        event_type="deposit",
        status=str(raw.get("status", "")),
        asset=str(raw.get("coin") or "") or None,
        asset_amount=str(raw.get("amount") or "") or None,
        network=str(raw.get("network") or "") or None,
        address=str(raw.get("address") or "") or None,
        address_tag=str(raw.get("addressTag") or "") or None,
        tx_id=str(raw.get("txId") or "") or None,
        transfer_id=str(raw.get("id") or "") or None,
        source_account="external",
        destination_account=_wallet_type_name(raw.get("walletType")),
        metadata_json=_json_metadata(
            {
                "transferType": raw.get("transferType"),
                "confirmTimes": raw.get("confirmTimes"),
                "unlockConfirm": raw.get("unlockConfirm"),
                "travelRuleStatus": raw.get("travelRuleStatus"),
            }
        ),
    )


def _normalize_withdrawal(config: BinanceConfig, raw: dict[str, Any]) -> BinanceAccountingEvent:
    timestamp = _parse_utc_datetime(raw.get("completeTime")) or _parse_utc_datetime(raw.get("applyTime")) or 0
    return BinanceAccountingEvent(
        event_id=f"withdrawal:{raw['id']}",
        exchange="binance",
        account_name=config.friendly_name,
        timestamp=timestamp,
        event_type="withdrawal",
        status=str(raw.get("status", "")),
        asset=str(raw.get("coin") or "") or None,
        asset_amount=_signed_amount(raw.get("amount", ""), -1),
        fee_asset=str(raw.get("coin") or "") or None,
        fee_amount=str(raw.get("transactionFee") or "") or None,
        network=str(raw.get("network") or "") or None,
        address=str(raw.get("address") or "") or None,
        tx_id=str(raw.get("txId") or "") or None,
        order_id=str(raw.get("withdrawOrderId") or "") or None,
        transfer_id=str(raw.get("id") or "") or None,
        source_account=_wallet_type_name(raw.get("walletType")),
        destination_account="external",
        metadata_json=_json_metadata(
            {
                "transferType": raw.get("transferType"),
                "confirmNo": raw.get("confirmNo"),
                "info": raw.get("info"),
                "txKey": raw.get("txKey"),
            }
        ),
    )


def _normalize_convert(config: BinanceConfig, raw: dict[str, Any]) -> BinanceAccountingEvent:
    return BinanceAccountingEvent(
        event_id=f"convert:{raw['orderId']}",
        exchange="binance",
        account_name=config.friendly_name,
        timestamp=int(raw["createTime"]),
        event_type="convert_trade",
        status=str(raw.get("orderStatus") or ""),
        asset=str(raw.get("toAsset") or "") or None,
        asset_amount=str(raw.get("toAmount") or "") or None,
        counter_asset=str(raw.get("fromAsset") or "") or None,
        counter_amount=_signed_amount(raw.get("fromAmount", ""), -1),
        quote_id=str(raw.get("quoteId") or "") or None,
        order_id=str(raw.get("orderId") or "") or None,
        reference=str(raw.get("ratio") or "") or None,
        metadata_json=_json_metadata(
            {
                "inverseRatio": raw.get("inverseRatio"),
            }
        ),
    )


def _normalize_dust_detail(
    config: BinanceConfig,
    parent: dict[str, Any],
    detail: dict[str, Any],
) -> BinanceAccountingEvent:
    return BinanceAccountingEvent(
        event_id=f"dust:{detail['transId']}:{detail['fromAsset']}",
        exchange="binance",
        account_name=config.friendly_name,
        timestamp=int(detail["operateTime"]),
        event_type="dust_conversion",
        status="success",
        asset=str(detail.get("fromAsset") or "") or None,
        asset_amount=_signed_amount(detail.get("amount", ""), -1),
        counter_asset="BNB",
        counter_amount=str(detail.get("transferedAmount") or "") or None,
        fee_asset="BNB",
        fee_amount=str(detail.get("serviceChargeAmount") or "") or None,
        transfer_id=str(detail.get("transId") or "") or None,
        source_account="spot",
        destination_account="spot",
        metadata_json=_json_metadata(
            {
                "totalTransferedAmount": parent.get("totalTransferedAmount"),
                "totalServiceChargeAmount": parent.get("totalServiceChargeAmount"),
            }
        ),
    )


def _normalize_dividend(config: BinanceConfig, raw: dict[str, Any]) -> BinanceAccountingEvent:
    return BinanceAccountingEvent(
        event_id=f"dividend:{raw['tranId']}:{raw['asset']}",
        exchange="binance",
        account_name=config.friendly_name,
        timestamp=int(raw["divTime"]),
        event_type="dividend",
        status="credited",
        asset=str(raw.get("asset") or "") or None,
        asset_amount=str(raw.get("amount") or "") or None,
        transfer_id=str(raw.get("tranId") or "") or None,
        reference=str(raw.get("enInfo") or "") or None,
        metadata_json=_json_metadata(
            {
                "direction": raw.get("direction"),
            }
        ),
    )


def _normalize_universal_transfer(
    config: BinanceConfig,
    transfer_type: str,
    raw: dict[str, Any],
) -> BinanceAccountingEvent:
    source_account = None
    destination_account = None
    if "_" in transfer_type:
        source_account, destination_account = transfer_type.split("_", 1)
        source_account = source_account.lower()
        destination_account = destination_account.lower()
    return BinanceAccountingEvent(
        event_id=f"universal_transfer:{raw['tranId']}:{transfer_type}",
        exchange="binance",
        account_name=config.friendly_name,
        timestamp=int(raw["timestamp"]),
        event_type="universal_transfer",
        status=str(raw.get("status") or ""),
        asset=str(raw.get("asset") or "") or None,
        asset_amount=str(raw.get("amount") or "") or None,
        source_account=source_account,
        destination_account=destination_account,
        transfer_id=str(raw.get("tranId") or "") or None,
        reference=transfer_type,
        metadata_json=_json_metadata({}),
    )


def _normalize_flexible_reward(config: BinanceConfig, raw: dict[str, Any]) -> BinanceAccountingEvent:
    return BinanceAccountingEvent(
        event_id=f"simple_earn_flexible:{raw['projectId']}:{raw['time']}:{raw['type']}",
        exchange="binance",
        account_name=config.friendly_name,
        timestamp=int(raw["time"]),
        event_type="simple_earn_flexible_reward",
        status="credited",
        asset=str(raw.get("asset") or "") or None,
        asset_amount=str(raw.get("rewards") or "") or None,
        reference=str(raw.get("projectId") or "") or None,
        metadata_json=_json_metadata(
            {
                "rewardType": raw.get("type"),
            }
        ),
    )


def _normalize_locked_reward(config: BinanceConfig, raw: dict[str, Any]) -> BinanceAccountingEvent:
    return BinanceAccountingEvent(
        event_id=f"simple_earn_locked:{raw['positionId']}:{raw['time']}:{raw['type']}",
        exchange="binance",
        account_name=config.friendly_name,
        timestamp=int(raw["time"]),
        event_type="simple_earn_locked_reward",
        status="credited",
        asset=str(raw.get("asset") or "") or None,
        asset_amount=str(raw.get("amount") or "") or None,
        reference=str(raw.get("positionId") or "") or None,
        metadata_json=_json_metadata(
            {
                "rewardType": raw.get("type"),
                "lockPeriod": raw.get("lockPeriod"),
            }
        ),
    )


def _update_state_time(state: dict[str, Any], section: str, key: str, value: int) -> None:
    if value <= 0:
        return
    bucket = state.setdefault(section, {})
    previous = _parse_int(bucket.get(key)) or 0
    if value > previous:
        bucket[key] = value


def _warn_universal_transfer_limit(config: BinanceConfig) -> None:
    logger.warning(
        "Binance universal transfer history for %s only supports the last 6 months via API",
        config.friendly_name,
    )


def _warn_dust_limit(config: BinanceConfig) -> None:
    logger.warning(
        "Binance dust log for %s only returns the last 100 records after 2020-12-01 via API",
        config.friendly_name,
    )


def sync_binance_account(
    config: BinanceConfig,
    output_dir: Path,
    api_key: str,
    api_secret: str,
    *,
    client: Optional[BinanceClient] = None,
    max_rows: Optional[int] = None,
) -> Path:
    """Sync Binance activity for one account into a normalized CSV."""
    active_client = client or BinanceClient(api_key=api_key, api_secret=api_secret)
    output_dir.mkdir(parents=True, exist_ok=True)

    csv_path = binance_csv_path(config, output_dir)
    state_path = binance_state_path(config, output_dir)
    state = _load_state(state_path)
    existing_event_ids = _existing_event_ids(csv_path)
    now_ms = int(time.time() * 1000)

    exchange_info = active_client.get_exchange_info()
    symbol_meta = {
        str(item.get("symbol")): item
        for item in exchange_info.get("symbols", [])
        if item.get("symbol")
    }
    for symbol in config.symbols:
        if symbol not in symbol_meta:
            logger.warning(
                "Configured Binance symbol %s for %s is not in current exchange info; attempting historical sync anyway",
                symbol,
                config.friendly_name,
            )

    events: dict[str, BinanceAccountingEvent] = {}

    try:
        for symbol in config.symbols:
            last_trade_id = _parse_int(state.get("spot_trades", {}).get(symbol, {}).get("last_trade_id"))
            next_from_id = 0 if last_trade_id is None else max(0, last_trade_id)
            while True:
                page = active_client.get_my_trades(symbol, from_id=next_from_id, limit=1000)
                if not page:
                    break
                max_trade_id = next_from_id
                for raw in page:
                    event = _normalize_trade(config, symbol_meta, raw)
                    trade_id = _parse_int(raw.get("id")) or 0
                    if trade_id >= max_trade_id:
                        max_trade_id = trade_id + 1
                    state.setdefault("spot_trades", {}).setdefault(symbol, {})["last_trade_id"] = max_trade_id
                    _, limit_reached = _store_event(events, existing_event_ids, event, max_rows)
                    if limit_reached:
                        raise BinanceRowLimitReached
                state.setdefault("spot_trades", {}).setdefault(symbol, {})["last_trade_id"] = max_trade_id
                if len(page) < 1000:
                    break
                next_from_id = max_trade_id

        deposit_start = _incremental_start(
            _parse_int(state.get("deposits", {}).get("last_complete_time_ms")),
            BINANCE_HISTORY_START_MS,
        )
        for window_start, window_end in _window_ranges(deposit_start, now_ms, DEPOSIT_WITHDRAW_WINDOW_MS):
            offset = 0
            while True:
                page = active_client.get_deposit_history(window_start, window_end, offset=offset, limit=1000)
                if not page:
                    break
                for raw in page:
                    event = _normalize_deposit(config, raw)
                    _update_state_time(state, "deposits", "last_complete_time_ms", event.timestamp + 1)
                    _, limit_reached = _store_event(events, existing_event_ids, event, max_rows)
                    if limit_reached:
                        raise BinanceRowLimitReached
                if len(page) < 1000:
                    break
                offset += 1000

        withdraw_start = _incremental_start(
            _parse_int(state.get("withdrawals", {}).get("last_complete_time_ms")),
            BINANCE_HISTORY_START_MS,
        )
        for window_start, window_end in _window_ranges(withdraw_start, now_ms, DEPOSIT_WITHDRAW_WINDOW_MS):
            offset = 0
            while True:
                page = active_client.get_withdraw_history(window_start, window_end, offset=offset, limit=1000)
                if not page:
                    break
                for raw in page:
                    event = _normalize_withdrawal(config, raw)
                    _update_state_time(state, "withdrawals", "last_complete_time_ms", event.timestamp + 1)
                    _, limit_reached = _store_event(events, existing_event_ids, event, max_rows)
                    if limit_reached:
                        raise BinanceRowLimitReached
                if len(page) < 1000:
                    break
                offset += 1000

        convert_start = _incremental_start(
            _parse_int(state.get("convert_trades", {}).get("last_create_time_ms")),
            BINANCE_HISTORY_START_MS,
        )
        for window_start, window_end in _window_ranges(convert_start, now_ms, CONVERT_WINDOW_MS):
            cursor_start = window_start
            while cursor_start <= window_end:
                payload = active_client.get_convert_trade_history(cursor_start, window_end, limit=100)
                rows = payload.get("list", [])
                if not rows:
                    break
                latest_time = cursor_start
                for raw in rows:
                    event = _normalize_convert(config, raw)
                    latest_time = max(latest_time, event.timestamp + 1)
                    _update_state_time(state, "convert_trades", "last_create_time_ms", event.timestamp + 1)
                    _, limit_reached = _store_event(events, existing_event_ids, event, max_rows)
                    if limit_reached:
                        raise BinanceRowLimitReached
                if not payload.get("moreData"):
                    break
                if latest_time <= cursor_start:
                    break
                cursor_start = latest_time

        if "universal_transfers" not in state:
            _warn_universal_transfer_limit(config)
        universal_default_start = max(now_ms - UNIVERSAL_TRANSFER_RETENTION_MS, BINANCE_HISTORY_START_MS)
        for transfer_type in config.transfer_types:
            key = transfer_type
            start_ms = _incremental_start(
                _parse_int(state.get("universal_transfers", {}).get(key, {}).get("last_timestamp_ms")),
                universal_default_start,
            )
            if start_ms < now_ms - UNIVERSAL_TRANSFER_RETENTION_MS:
                _warn_universal_transfer_limit(config)
                start_ms = universal_default_start
            current = 1
            while True:
                payload = active_client.get_universal_transfer_history(
                    transfer_type,
                    start_time=start_ms,
                    end_time=now_ms,
                    current=current,
                    size=100,
                )
                rows = payload.get("rows", [])
                if not rows:
                    break
                for raw in rows:
                    event = _normalize_universal_transfer(config, transfer_type, raw)
                    transfer_state = state.setdefault("universal_transfers", {}).setdefault(key, {})
                    previous_timestamp = _parse_int(transfer_state.get("last_timestamp_ms")) or 0
                    if event.timestamp + 1 > previous_timestamp:
                        transfer_state["last_timestamp_ms"] = event.timestamp + 1
                    _, limit_reached = _store_event(events, existing_event_ids, event, max_rows)
                    if limit_reached:
                        raise BinanceRowLimitReached
                total = _parse_int(payload.get("total")) or len(rows)
                if current * 100 >= total or len(rows) < 100:
                    break
                current += 1

        if "dust_log" not in state:
            _warn_dust_limit(config)
        dust_start = _incremental_start(
            _parse_int(state.get("dust_log", {}).get("last_operate_time_ms")),
            DUST_HISTORY_START_MS,
        )
        dust_payload = active_client.get_dust_log(start_time=dust_start, end_time=now_ms)
        user_asset_dribblets = dust_payload.get("userAssetDribblets", [])
        total_dribblets = _parse_int(dust_payload.get("total")) or len(user_asset_dribblets)
        if total_dribblets > len(user_asset_dribblets):
            _warn_dust_limit(config)
        for parent in user_asset_dribblets:
            for detail in parent.get("userAssetDribbletDetails", []):
                event = _normalize_dust_detail(config, parent, detail)
                _update_state_time(state, "dust_log", "last_operate_time_ms", event.timestamp + 1)
                _, limit_reached = _store_event(events, existing_event_ids, event, max_rows)
                if limit_reached:
                    raise BinanceRowLimitReached

        dividend_start = _incremental_start(
            _parse_int(state.get("dividends", {}).get("last_div_time_ms")),
            BINANCE_HISTORY_START_MS,
        )
        for window_start, window_end in _window_ranges(dividend_start, now_ms, DIVIDEND_WINDOW_MS):
            current = 1
            while True:
                payload = active_client.get_asset_dividend_history(window_start, window_end, current=current, limit=500)
                rows = payload.get("rows", [])
                if not rows:
                    break
                for raw in rows:
                    event = _normalize_dividend(config, raw)
                    _update_state_time(state, "dividends", "last_div_time_ms", event.timestamp + 1)
                    _, limit_reached = _store_event(events, existing_event_ids, event, max_rows)
                    if limit_reached:
                        raise BinanceRowLimitReached
                total = _parse_int(payload.get("total")) or len(rows)
                if current * 500 >= total or len(rows) < 500:
                    break
                current += 1

        flexible_start = _incremental_start(
            _parse_int(state.get("simple_earn_flexible", {}).get("last_time_ms")),
            BINANCE_HISTORY_START_MS,
        )
        for window_start, window_end in _window_ranges(flexible_start, now_ms, SIMPLE_EARN_WINDOW_MS):
            current = 1
            while True:
                payload = active_client.get_flexible_rewards_history(window_start, window_end, current=current, size=100, reward_type="ALL")
                rows = payload.get("rows", [])
                if not rows:
                    break
                for raw in rows:
                    event = _normalize_flexible_reward(config, raw)
                    _update_state_time(state, "simple_earn_flexible", "last_time_ms", event.timestamp + 1)
                    _, limit_reached = _store_event(events, existing_event_ids, event, max_rows)
                    if limit_reached:
                        raise BinanceRowLimitReached
                total = _parse_int(payload.get("total")) or len(rows)
                if current * 100 >= total or len(rows) < 100:
                    break
                current += 1

        locked_start = _incremental_start(
            _parse_int(state.get("simple_earn_locked", {}).get("last_time_ms")),
            BINANCE_HISTORY_START_MS,
        )
        for window_start, window_end in _window_ranges(locked_start, now_ms, SIMPLE_EARN_WINDOW_MS):
            current = 1
            while True:
                payload = active_client.get_locked_rewards_history(window_start, window_end, current=current, size=100)
                rows = payload.get("rows", [])
                if not rows:
                    break
                for raw in rows:
                    event = _normalize_locked_reward(config, raw)
                    _update_state_time(state, "simple_earn_locked", "last_time_ms", event.timestamp + 1)
                    _, limit_reached = _store_event(events, existing_event_ids, event, max_rows)
                    if limit_reached:
                        raise BinanceRowLimitReached
                total = _parse_int(payload.get("total")) or len(rows)
                if current * 100 >= total or len(rows) < 100:
                    break
                current += 1
    except BinanceRowLimitReached:
        logger.info(
            "Stopped Binance sync for %s after reaching requested row limit of %d",
            config.friendly_name,
            max_rows,
        )

    path = _write_events(events.values(), config, output_dir)
    _save_state(state_path, state)
    return path
