"""Tests for Binance sync and normalized CSV export."""
import csv
import json
import logging
from unittest.mock import patch

from sync_crypto.models import BinanceConfig


class FakeBinanceClient:
    def get_exchange_info(self):
        return {
            "symbols": [
                {
                    "symbol": "BTCUSDT",
                    "baseAsset": "BTC",
                    "quoteAsset": "USDT",
                }
            ]
        }

    def get_my_trades(self, symbol, *, from_id=None, order_id=None, limit=1000):
        if symbol == "BTCUSDT" and from_id == 0:
            return [
                {
                    "symbol": "BTCUSDT",
                    "id": 7,
                    "orderId": 9,
                    "orderListId": -1,
                    "price": "50000.00",
                    "qty": "0.10000000",
                    "quoteQty": "5000.00000000",
                    "commission": "0.00100000",
                    "commissionAsset": "BNB",
                    "time": 1500,
                    "isBuyer": True,
                    "isMaker": False,
                    "isBestMatch": True,
                }
            ]
        return []

    def get_deposit_history(self, start_time, end_time, *, offset=0, limit=1000):
        if offset == 0 and start_time <= 1510 <= end_time:
            return [
                {
                    "id": "dep-1",
                    "amount": "1.50000000",
                    "coin": "USDT",
                    "network": "ETH",
                    "status": 1,
                    "address": "0xdeposit",
                    "addressTag": "",
                    "txId": "0xdep",
                    "insertTime": 1510,
                    "completeTime": 1510,
                    "transferType": 0,
                    "confirmTimes": "12/12",
                    "unlockConfirm": 0,
                    "walletType": 0,
                    "travelRuleStatus": 0,
                }
            ]
        return []

    def get_withdraw_history(self, start_time, end_time, *, offset=0, limit=1000):
        if offset == 0 and start_time <= 1520 <= end_time:
            return [
                {
                    "id": "wd-1",
                    "amount": "0.25000000",
                    "transactionFee": "0.00100000",
                    "coin": "BTC",
                    "status": 6,
                    "address": "1withdraw",
                    "txId": "0xwd",
                    "applyTime": "1970-01-01 00:00:01",
                    "network": "BTC",
                    "transferType": 0,
                    "info": "",
                    "confirmNo": 2,
                    "walletType": 1,
                    "txKey": "",
                    "completeTime": "1970-01-01 00:00:01",
                    "withdrawOrderId": "binance-order",
                }
            ]
        return []

    def get_convert_trade_history(self, start_time, end_time, *, limit=100):
        if start_time <= 1530 <= end_time:
            return {
                "list": [
                    {
                        "quoteId": "quote-1",
                        "orderId": 44,
                        "orderStatus": "SUCCESS",
                        "fromAsset": "USDT",
                        "fromAmount": "20",
                        "toAsset": "BNB",
                        "toAmount": "0.06154036",
                        "ratio": "0.00307702",
                        "inverseRatio": "324.99",
                        "createTime": 1530,
                    }
                ],
                "moreData": False,
            }
        return {"list": [], "moreData": False}

    def get_dust_log(self, *, start_time=None, end_time=None, account_type="SPOT"):
        if start_time <= 1540 <= end_time:
            return {
                "total": 1,
                "userAssetDribblets": [
                    {
                        "operateTime": 1540,
                        "totalTransferedAmount": "0.00049",
                        "totalServiceChargeAmount": "0.00001",
                        "transId": 55,
                        "userAssetDribbletDetails": [
                            {
                                "transId": 55,
                                "serviceChargeAmount": "0.00001",
                                "amount": "0.001",
                                "operateTime": 1540,
                                "transferedAmount": "0.00049",
                                "fromAsset": "ETH",
                            }
                        ],
                    }
                ],
            }
        return {"total": 0, "userAssetDribblets": []}

    def get_asset_dividend_history(self, start_time, end_time, *, current=1, limit=500):
        if current == 1 and start_time <= 1550 <= end_time:
            return {
                "total": 1,
                "rows": [
                    {
                        "amount": "10.00000000",
                        "asset": "BHFT",
                        "divTime": 1550,
                        "enInfo": "BHFT distribution",
                        "tranId": 999,
                        "direction": 1,
                    }
                ],
            }
        return {"total": 0, "rows": []}

    def get_universal_transfer_history(
        self,
        transfer_type,
        *,
        start_time,
        end_time,
        current=1,
        size=100,
    ):
        if transfer_type == "MAIN_FUNDING" and current == 1 and start_time <= 1560 <= end_time:
            return {
                "total": 1,
                "rows": [
                    {
                        "asset": "USDT",
                        "amount": "25",
                        "type": "MAIN_FUNDING",
                        "status": "CONFIRMED",
                        "tranId": 123,
                        "timestamp": 1560,
                    }
                ],
            }
        return {"total": 0, "rows": []}

    def get_flexible_rewards_history(
        self,
        start_time,
        end_time,
        *,
        current=1,
        size=100,
        reward_type="ALL",
    ):
        if current == 1 and start_time <= 1570 <= end_time:
            return {
                "total": 1,
                "rows": [
                    {
                        "asset": "USDT",
                        "rewards": "0.00687654",
                        "projectId": "USDT001",
                        "type": "REALTIME",
                        "time": 1570,
                    }
                ],
            }
        return {"total": 0, "rows": []}

    def get_locked_rewards_history(self, start_time, end_time, *, current=1, size=100):
        if current == 1 and start_time <= 1580 <= end_time:
            return {
                "total": 1,
                "rows": [
                    {
                        "positionId": 321,
                        "time": 1580,
                        "asset": "BNB",
                        "lockPeriod": "30",
                        "amount": "1.23223",
                        "type": "Locked Rewards",
                    }
                ],
            }
        return {"total": 0, "rows": []}


class WarningBinanceClient(FakeBinanceClient):
    def get_exchange_info(self):
        return {"symbols": []}

    def get_my_trades(self, symbol, *, from_id=None, order_id=None, limit=1000):
        return []

    def get_deposit_history(self, start_time, end_time, *, offset=0, limit=1000):
        return []

    def get_withdraw_history(self, start_time, end_time, *, offset=0, limit=1000):
        return []

    def get_convert_trade_history(self, start_time, end_time, *, limit=100):
        return {"list": [], "moreData": False}

    def get_dust_log(self, *, start_time=None, end_time=None, account_type="SPOT"):
        return {"total": 101, "userAssetDribblets": []}

    def get_asset_dividend_history(self, start_time, end_time, *, current=1, limit=500):
        return {"total": 0, "rows": []}

    def get_universal_transfer_history(
        self,
        transfer_type,
        *,
        start_time,
        end_time,
        current=1,
        size=100,
    ):
        return {"total": 0, "rows": []}

    def get_flexible_rewards_history(
        self,
        start_time,
        end_time,
        *,
        current=1,
        size=100,
        reward_type="ALL",
    ):
        return {"total": 0, "rows": []}

    def get_locked_rewards_history(self, start_time, end_time, *, current=1, size=100):
        return {"total": 0, "rows": []}


class TestBinanceClient:
    @patch("sync_crypto.binance.time.time", return_value=1.0)
    @patch("sync_crypto.binance.time.sleep")
    @patch("sync_crypto.binance.requests.get")
    def test_signed_request_retries_on_429(self, mock_get, mock_sleep, _time):
        from sync_crypto.binance import BinanceClient

        first = type("Response", (), {})()
        first.status_code = 429
        first.json = lambda: {"code": -1003}
        first.raise_for_status = lambda: None

        second = type("Response", (), {})()
        second.status_code = 200
        second.json = lambda: {"ok": True}
        second.raise_for_status = lambda: None

        mock_get.side_effect = [first, second]

        client = BinanceClient(api_key="key", api_secret="secret")
        with patch.object(client.rate_limiter, "wait"):
            result = client._request_signed("/sapi/test", {"foo": "bar"})

        assert result == {"ok": True}
        assert mock_get.call_count == 2
        mock_sleep.assert_called_once_with(6)


class TestSyncBinanceAccount:
    @patch("sync_crypto.binance.time.time", return_value=2.0)
    @patch("sync_crypto.binance.DUST_HISTORY_START_MS", 1000)
    @patch("sync_crypto.binance.BINANCE_HISTORY_START_MS", 1000)
    def test_sync_binance_account_writes_csv_and_state(self, _time, tmp_path):
        from sync_crypto.binance import binance_state_path, sync_binance_account

        config = BinanceConfig(
            exchange="binance",
            friendly_name="main_binance",
            symbols=["BTCUSDT"],
        )

        first_output = sync_binance_account(
            config,
            tmp_path,
            api_key="key",
            api_secret="secret",
            client=FakeBinanceClient(),
        )
        second_output = sync_binance_account(
            config,
            tmp_path,
            api_key="key",
            api_secret="secret",
            client=FakeBinanceClient(),
        )

        assert first_output == second_output
        assert first_output.name == "binance_main_binance_accounting.csv"

        with open(first_output, newline="") as handle:
            rows = list(csv.DictReader(handle))

        assert len(rows) == 9
        assert {row["event_type"] for row in rows} == {
            "spot_trade",
            "deposit",
            "withdrawal",
            "convert_trade",
            "dust_conversion",
            "dividend",
            "universal_transfer",
            "simple_earn_flexible_reward",
            "simple_earn_locked_reward",
        }

        trade_row = next(row for row in rows if row["event_type"] == "spot_trade")
        assert trade_row["asset"] == "BTC"
        assert trade_row["asset_amount"] == "0.10000000"
        assert trade_row["counter_asset"] == "USDT"
        assert trade_row["counter_amount"] == "-5000.00000000"
        assert trade_row["fee_asset"] == "BNB"
        assert trade_row["trade_id"] == "7"

        state_path = binance_state_path(config, tmp_path)
        state = json.loads(state_path.read_text())
        assert state["spot_trades"]["BTCUSDT"]["last_trade_id"] == 8
        assert state["deposits"]["last_complete_time_ms"] == 1511
        assert state["dust_log"]["last_operate_time_ms"] == 1541

    @patch("sync_crypto.binance.time.time", return_value=2.0)
    @patch("sync_crypto.binance.DUST_HISTORY_START_MS", 1000)
    @patch("sync_crypto.binance.BINANCE_HISTORY_START_MS", 1000)
    def test_sync_binance_account_respects_max_rows(self, _time, tmp_path, caplog):
        from sync_crypto.binance import binance_state_path, sync_binance_account

        caplog.set_level(logging.INFO, logger="sync_crypto.binance")

        config = BinanceConfig(
            exchange="binance",
            friendly_name="main_binance",
            symbols=["BTCUSDT"],
        )

        output = sync_binance_account(
            config,
            tmp_path,
            api_key="key",
            api_secret="secret",
            client=FakeBinanceClient(),
            max_rows=2,
        )

        with open(output, newline="") as handle:
            rows = list(csv.DictReader(handle))

        assert len(rows) == 2
        assert [row["event_type"] for row in rows] == ["spot_trade", "deposit"]
        assert "requested row limit of 2" in caplog.text

        state = json.loads(binance_state_path(config, tmp_path).read_text())
        assert state["spot_trades"]["BTCUSDT"]["last_trade_id"] == 8
        assert state["deposits"]["last_complete_time_ms"] == 1511
        assert "withdrawals" not in state

    @patch("sync_crypto.binance.time.time", return_value=2.0)
    @patch("sync_crypto.binance.DUST_HISTORY_START_MS", 1000)
    @patch("sync_crypto.binance.BINANCE_HISTORY_START_MS", 1000)
    def test_sync_binance_account_max_rows_only_counts_new_csv_rows(self, _time, tmp_path, caplog):
        from sync_crypto.binance import binance_state_path, sync_binance_account

        caplog.set_level(logging.INFO, logger="sync_crypto.binance")

        config = BinanceConfig(
            exchange="binance",
            friendly_name="main_binance",
            symbols=["BTCUSDT"],
        )

        first_output = sync_binance_account(
            config,
            tmp_path,
            api_key="key",
            api_secret="secret",
            client=FakeBinanceClient(),
            max_rows=2,
        )
        state_path = binance_state_path(config, tmp_path)
        state_path.write_text("{}")

        second_output = sync_binance_account(
            config,
            tmp_path,
            api_key="key",
            api_secret="secret",
            client=FakeBinanceClient(),
            max_rows=2,
        )

        assert first_output == second_output

        with open(second_output, newline="") as handle:
            rows = list(csv.DictReader(handle))

        assert len(rows) == 4
        assert [row["event_type"] for row in rows] == [
            "spot_trade",
            "deposit",
            "withdrawal",
            "convert_trade",
        ]
        assert caplog.text.count("requested row limit of 2") == 2

        state = json.loads(state_path.read_text())
        assert state["withdrawals"]["last_complete_time_ms"] == 1001
        assert state["convert_trades"]["last_create_time_ms"] == 1531
        assert "dust_log" not in state

    @patch("sync_crypto.binance.time.time", return_value=2.0)
    @patch("sync_crypto.binance.DUST_HISTORY_START_MS", 1000)
    @patch("sync_crypto.binance.BINANCE_HISTORY_START_MS", 1000)
    def test_sync_binance_account_reports_limitations(self, _time, tmp_path, caplog):
        from sync_crypto.binance import sync_binance_account

        config = BinanceConfig(
            exchange="binance",
            friendly_name="main_binance",
            symbols=["BTCUSDT"],
        )

        sync_binance_account(
            config,
            tmp_path,
            api_key="key",
            api_secret="secret",
            client=WarningBinanceClient(),
        )

        assert "last 6 months via API" in caplog.text
        assert "last 100 records after 2020-12-01 via API" in caplog.text
