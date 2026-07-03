"""Tests for sync_crypto.sync orchestration."""
import csv
import json
from pathlib import Path
from unittest.mock import patch, MagicMock

import pytest

from sync_crypto.models import BinanceConfig, TransactionRecord, WalletConfig
from sync_crypto.sync import CSV_FIELDS

FIXTURES = Path(__file__).parent / "fixtures"


class TestLoadConfig:
    def test_load_valid_config(self):
        from sync_crypto.sync import load_config

        wallets = load_config(FIXTURES / "wallets_config.json")
        assert len(wallets) == 2
        assert wallets[0].blockchain == "ethereum"
        assert wallets[0].friendly_name == "main_eth"
        assert wallets[1].blockchain == "solana"

    def test_load_missing_file_raises(self):
        from sync_crypto.sync import load_config

        with pytest.raises(FileNotFoundError):
            load_config(Path("/nonexistent/wallets.json"))

    def test_load_invalid_json_raises(self, tmp_path):
        from sync_crypto.sync import load_config

        bad_file = tmp_path / "bad.json"
        bad_file.write_text("not json")
        with pytest.raises(json.JSONDecodeError):
            load_config(bad_file)

    def test_load_invalid_wallet_raises(self, tmp_path):
        from sync_crypto.sync import load_config

        bad_file = tmp_path / "bad_wallet.json"
        bad_file.write_text(json.dumps([
            {"blockchain": "bitcoin", "friendly_name": "btc", "address": "1A1"}
        ]))
        with pytest.raises(ValueError):
            load_config(bad_file)

    def test_load_mixed_wallet_and_binance_sources(self):
        from sync_crypto.sync import load_config

        sources = load_config(FIXTURES / "mixed_sync_sources.json")

        assert len(sources) == 2
        assert isinstance(sources[0], WalletConfig)
        assert isinstance(sources[1], BinanceConfig)
        assert sources[1].exchange == "binance"
        assert sources[1].symbols == ["BTCUSDT", "ETHUSDT"]

    def test_load_invalid_binance_source_raises(self, tmp_path):
        from sync_crypto.sync import load_config

        bad_file = tmp_path / "bad_binance.json"
        bad_file.write_text(json.dumps([
            {"exchange": "binance", "friendly_name": "binance", "symbols": []}
        ]))
        with pytest.raises(ValueError):
            load_config(bad_file)

    def test_load_zerion_wallet_source(self, tmp_path):
        from sync_crypto.sync import load_config

        config_file = tmp_path / "wallets.json"
        config_file.write_text(json.dumps([
            {
                "blockchain": "ethereum",
                "friendly_name": "main_eth",
                "address": "0xabc",
                "provider": "zerion",
                "network": "base",
            }
        ]))

        sources = load_config(config_file)

        assert len(sources) == 1
        assert sources[0].provider == "zerion"
        assert sources[0].network == "base"

    def test_load_grouped_wallet_config_object(self, tmp_path):
        from sync_crypto.sync import load_config

        config_file = tmp_path / "wallets.json"
        config_file.write_text(json.dumps({
            "solana": {
                "friendly_name": "sol_main",
                "address": "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
            },
            "ethereum": [
                {
                    "friendly_name": "main_eth",
                    "address": "0xde0B295669a9FD93d5F28D9Ec85E40f4cb697BAe",
                },
                {
                    "friendly_name": "base_wallet",
                    "address": "0xabc",
                    "provider": "zerion",
                    "network": "base",
                },
            ],
        }))

        sources = load_config(config_file)

        assert len(sources) == 3
        assert isinstance(sources[0], WalletConfig)
        assert sources[0].blockchain == "solana"
        assert sources[1].blockchain == "ethereum"
        assert sources[2].provider == "zerion"
        assert sources[2].network == "base"

    def test_load_grouped_config_with_binance(self, tmp_path):
        from sync_crypto.sync import load_config

        config_file = tmp_path / "wallets.json"
        config_file.write_text(json.dumps({
            "ethereum": {
                "friendly_name": "main_eth",
                "address": "0xde0B295669a9FD93d5F28D9Ec85E40f4cb697BAe",
            },
            "binance": {
                "friendly_name": "main_binance",
                "symbols": ["BTCUSDT"],
            },
        }))

        sources = load_config(config_file)

        assert len(sources) == 2
        assert isinstance(sources[0], WalletConfig)
        assert isinstance(sources[1], BinanceConfig)


class TestWriteCsv:
    def test_write_csv_creates_file(self, tmp_path):
        from sync_crypto.sync import write_csv

        txs = [
            TransactionRecord(
                tx_hash="0xabc",
                blockchain="ethereum",
                timestamp=1700000000,
                from_address="0xfrom",
                to_address="0xto",
                value="1000",
                fee="21000",
                status="success",
                block_number=18500000,
            ),
            TransactionRecord(
                tx_hash="0xdef",
                blockchain="ethereum",
                timestamp=1700000100,
                from_address="0xfrom",
                to_address="0xto2",
                value="2000",
                fee="42000",
                status="success",
            ),
        ]
        output_path = write_csv(txs, "ethereum", "0xabc123", tmp_path)
        assert output_path.exists()
        assert output_path.name == "ethereum_0xabc123_transactions.csv"

        content = output_path.read_text()
        lines = content.strip().split("\n")
        assert len(lines) == 3  # header + 2 rows
        assert "tx_hash" in lines[0]
        assert "0xabc" in lines[1]
        assert "0xdef" in lines[2]

    def test_write_csv_empty_transactions(self, tmp_path):
        from sync_crypto.sync import write_csv

        output_path = write_csv([], "ethereum", "0xempty", tmp_path)
        assert output_path.exists()
        content = output_path.read_text()
        lines = content.strip().split("\n")
        assert len(lines) == 1  # header only

    def test_write_csv_sorted_by_timestamp(self, tmp_path):
        from sync_crypto.sync import write_csv

        txs = [
            TransactionRecord(
                tx_hash="0xlater",
                blockchain="ethereum",
                timestamp=1700000200,
                from_address="0xfrom",
                to_address="0xto",
                value="1000",
                fee="21000",
                status="success",
            ),
            TransactionRecord(
                tx_hash="0xearlier",
                blockchain="ethereum",
                timestamp=1700000000,
                from_address="0xfrom",
                to_address="0xto",
                value="2000",
                fee="21000",
                status="success",
            ),
        ]
        output_path = write_csv(txs, "ethereum", "0xsorted", tmp_path)
        content = output_path.read_text()
        lines = content.strip().split("\n")
        assert "0xearlier" in lines[1]
        assert "0xlater" in lines[2]

    def test_write_csv_append_deduplicates_by_record_id(self, tmp_path):
        from sync_crypto.sync import write_csv

        existing = TransactionRecord(
            record_id="zerion:tx-1:0",
            provider="zerion",
            network="ethereum",
            tx_hash="0xshared",
            blockchain="ethereum",
            timestamp=1700000000,
            from_address="0xfrom",
            to_address="0xto",
            value="1000",
            fee="21000",
            status="success",
        )
        write_csv([existing], "ethereum", "0xdup", tmp_path)

        duplicate = TransactionRecord(
            record_id="zerion:tx-1:0",
            provider="zerion",
            network="ethereum",
            tx_hash="0xshared",
            blockchain="ethereum",
            timestamp=1700000000,
            from_address="0xfrom",
            to_address="0xto",
            value="1000",
            fee="21000",
            status="success",
        )
        distinct = TransactionRecord(
            record_id="zerion:tx-1:1",
            provider="zerion",
            network="ethereum",
            tx_hash="0xshared",
            blockchain="ethereum",
            timestamp=1700000001,
            from_address="0xfrom",
            to_address="0xto2",
            value="2000",
            fee="21000",
            status="success",
        )

        output_path = write_csv([duplicate, distinct], "ethereum", "0xdup", tmp_path, append=True)

        rows = list(csv.DictReader(output_path.open()))
        assert len(rows) == 2
        assert rows[1]["record_id"] == "zerion:tx-1:1"


class TestSyncWallet:
    @patch("sync_crypto.sync.EtherscanClient")
    def test_sync_ethereum_wallet(self, mock_eth_cls, tmp_path):
        from sync_crypto.sync import sync_wallet

        mock_client = MagicMock()
        mock_client.fetch_all_transactions.return_value = [
            TransactionRecord(
                tx_hash="0xtest",
                blockchain="ethereum",
                timestamp=1700000000,
                from_address="0xfrom",
                to_address="0xto",
                value="1000",
                fee="21000",
                status="success",
            )
        ]
        mock_eth_cls.return_value = mock_client

        wallet = WalletConfig(
            blockchain="ethereum",
            friendly_name="test_eth",
            address="0xaddr",
        )
        output = sync_wallet(wallet, tmp_path, etherscan_key="test_key")
        assert output.exists()
        assert output.name == "ethereum_0xaddr_transactions.csv"
        mock_client.fetch_all_transactions.assert_called_once_with("0xaddr", start_block=0)

    @patch("sync_crypto.sync.SolanaRpcClient")
    def test_sync_solana_wallet_chunked(self, mock_sol_cls, tmp_path):
        from sync_crypto.sync import sync_wallet

        tx1 = TransactionRecord(
            tx_hash="5test1", blockchain="solana", timestamp=1700000000,
            from_address="7xK", to_address="8yL", value="1000000000",
            fee="5000", status="success",
        )
        tx2 = TransactionRecord(
            tx_hash="5test2", blockchain="solana", timestamp=1700000100,
            from_address="8yL", to_address="7xK", value="500000000",
            fee="5000", status="success",
        )
        mock_client = MagicMock()
        mock_client.fetch_transactions_chunked.return_value = iter([[tx1], [tx2]])
        mock_sol_cls.return_value = mock_client

        wallet = WalletConfig(
            blockchain="solana", friendly_name="test_sol", address="7xKaddr",
        )
        output = sync_wallet(wallet, tmp_path)
        assert output.exists()
        assert output.name == "solana_7xKaddr_transactions.csv"

        content = output.read_text()
        lines = content.strip().split("\n")
        assert len(lines) == 3  # header + 2 rows
        assert "5test1" in lines[1]
        assert "5test2" in lines[2]

    @patch("sync_crypto.sync.EtherscanClient")
    def test_sync_wallet_error_returns_none(self, mock_eth_cls, tmp_path):
        from sync_crypto.sync import sync_wallet

        mock_client = MagicMock()
        mock_client.fetch_all_transactions.side_effect = Exception("API error")
        mock_eth_cls.return_value = mock_client

        wallet = WalletConfig(
            blockchain="ethereum",
            friendly_name="error_eth",
            address="0xaddr",
        )
        result = sync_wallet(wallet, tmp_path, etherscan_key="test_key")
        assert result is None

    @patch("sync_crypto.sync.ZerionClient")
    def test_sync_zerion_wallet_writes_csv_and_state(self, mock_zerion_cls, tmp_path):
        from sync_crypto.sync import sync_wallet, wallet_state_path

        tx1 = TransactionRecord(
            record_id="zerion:tx-1:0",
            provider="zerion",
            network="ethereum",
            tx_hash="0xzerion1",
            blockchain="ethereum",
            timestamp=1700000000,
            from_address="0xfrom",
            to_address="0xto",
            value="1000",
            fee="21000",
            status="success",
        )
        tx2 = TransactionRecord(
            record_id="zerion:tx-2:0",
            provider="zerion",
            network="ethereum",
            tx_hash="0xzerion2",
            blockchain="ethereum",
            timestamp=1700000100,
            from_address="0xfrom",
            to_address="0xto2",
            value="2000",
            fee="21000",
            status="success",
        )
        mock_client = MagicMock()
        mock_client.fetch_new_transaction_batches.return_value = iter([[tx1], [tx2]])
        mock_client.advance_state.side_effect = [
            {
                "provider": "zerion",
            },
            {
                "provider": "zerion",
                "latest_timestamp": 1700000000,
                "latest_record_ids": ["zerion:tx-1:0"],
            },
            {
                "provider": "zerion",
                "latest_timestamp": 1700000100,
                "latest_record_ids": ["zerion:tx-2:0"],
            },
        ]
        mock_zerion_cls.return_value = mock_client

        wallet = WalletConfig(
            blockchain="ethereum",
            friendly_name="test_eth",
            address="0xaddr",
            provider="zerion",
            network="ethereum",
        )

        output = sync_wallet(wallet, tmp_path, zerion_api_key="zk_dev_test")

        assert output.exists()
        rows = list(csv.DictReader(output.open()))
        assert len(rows) == 2
        assert rows[0]["record_id"] == "zerion:tx-1:0"
        assert rows[1]["record_id"] == "zerion:tx-2:0"
        state = json.loads(wallet_state_path(wallet, tmp_path).read_text())
        assert state == {
            "provider": "zerion",
            "latest_timestamp": 1700000100,
            "latest_record_ids": ["zerion:tx-2:0"],
        }
        mock_client.fetch_new_transaction_batches.assert_called_once()

    @patch("sync_crypto.sync.ZerionClient")
    def test_sync_ethereum_zerion_wallet_passes_etherscan_api_key(self, mock_zerion_cls, tmp_path):
        from sync_crypto.sync import sync_wallet

        mock_client = MagicMock()
        mock_client.fetch_new_transaction_batches.return_value = iter([])
        mock_client.advance_state.return_value = {"provider": "zerion"}
        mock_zerion_cls.return_value = mock_client

        wallet = WalletConfig(
            blockchain="ethereum",
            friendly_name="test_eth",
            address="0xaddr",
            provider="zerion",
            network="ethereum",
        )

        sync_wallet(
            wallet,
            tmp_path,
            etherscan_key="eth_test_key",
            zerion_api_key="zk_dev_test",
        )

        assert mock_zerion_cls.call_args.kwargs["etherscan_api_key"] == "eth_test_key"

    @patch("sync_crypto.sync.ZerionClient")
    def test_sync_zerion_wallet_keeps_partial_csv_when_streaming_fails(self, mock_zerion_cls, tmp_path):
        from sync_crypto.sync import sync_wallet, wallet_state_path

        tx = TransactionRecord(
            record_id="zerion:tx-1:0",
            provider="zerion",
            network="ethereum",
            tx_hash="0xzerion1",
            blockchain="ethereum",
            timestamp=1700000000,
            from_address="0xfrom",
            to_address="0xto",
            value="1000",
            fee="21000",
            status="success",
        )

        def failing_batches():
            yield [tx]
            raise RuntimeError("boom")

        mock_client = MagicMock()
        mock_client.fetch_new_transaction_batches.return_value = failing_batches()
        mock_client.advance_state.side_effect = [
            {"provider": "zerion"},
            {
                "provider": "zerion",
                "latest_timestamp": 1700000000,
                "latest_record_ids": ["zerion:tx-1:0"],
            },
        ]
        mock_zerion_cls.return_value = mock_client

        wallet = WalletConfig(
            blockchain="ethereum",
            friendly_name="test_eth",
            address="0xaddr",
            provider="zerion",
            network="ethereum",
        )

        result = sync_wallet(wallet, tmp_path, zerion_api_key="zk_dev_test")

        assert result is None
        csv_path = tmp_path / "ethereum_0xaddr_transactions.csv"
        rows = list(csv.DictReader(csv_path.open()))
        assert len(rows) == 1
        assert rows[0]["record_id"] == "zerion:tx-1:0"
        assert not wallet_state_path(wallet, tmp_path).exists()

    @patch("sync_crypto.sync.EtherscanClient")
    @patch("sync_crypto.sync.ZerionClient")
    def test_sync_ethereum_zerion_supplements_native_eth_from_etherscan(
        self, mock_zerion_cls, mock_eth_cls, tmp_path
    ):
        """Zerion misses native ETH transfers; they should be backfilled from Etherscan."""
        from sync_crypto.sync import sync_wallet

        # Zerion returns one token transfer
        zerion_tx = TransactionRecord(
            record_id="zerion:tx-1:0",
            provider="zerion",
            network="ethereum",
            tx_hash="0xzerion1",
            blockchain="ethereum",
            timestamp=1700000000,
            from_address="0xfrom",
            to_address="0xaddr",
            value="1000",
            fee="21000",
            status="success",
            tx_type="token_transfer",
            token_symbol="USDC",
        )
        mock_zerion = MagicMock()
        mock_zerion.fetch_new_transaction_batches.return_value = iter([[zerion_tx]])
        mock_zerion.advance_state.side_effect = [
            {"provider": "zerion"},
            {"provider": "zerion", "latest_timestamp": 1700000000, "latest_record_ids": ["zerion:tx-1:0"]},
        ]
        mock_zerion_cls.return_value = mock_zerion

        # Etherscan returns a normal ETH transfer (different tx_hash)
        eth_normal_tx = TransactionRecord(
            tx_hash="0xeth_normal1",
            blockchain="ethereum",
            timestamp=1699999000,
            from_address="0xsender",
            to_address="0xaddr",
            value="500000000000000000",
            fee="21000",
            status="success",
            tx_type="normal",
            currency="ETH",
            gas_used="21000",
            gas_price="1000000000",
        )
        # Etherscan also returns an internal ETH transfer
        eth_internal_tx = TransactionRecord(
            tx_hash="0xeth_internal1",
            blockchain="ethereum",
            timestamp=1699998000,
            from_address="0xcontract",
            to_address="0xaddr",
            value="100000000000000000",
            fee="0",
            status="success",
            tx_type="internal",
            currency="ETH",
        )
        mock_eth = MagicMock()
        mock_eth._fetch_normal_transactions.return_value = [eth_normal_tx]
        mock_eth._fetch_internal_transactions.return_value = [eth_internal_tx]
        mock_eth_cls.return_value = mock_eth

        wallet = WalletConfig(
            blockchain="ethereum",
            friendly_name="test_eth",
            address="0xaddr",
            provider="zerion",
            network="ethereum",
        )

        output = sync_wallet(
            wallet, tmp_path, etherscan_key="eth_key", zerion_api_key="zk_key"
        )

        assert output.exists()
        rows = list(csv.DictReader(output.open()))
        tx_types = [r["tx_type"] for r in rows]
        assert "token_transfer" in tx_types
        assert "normal" in tx_types
        assert "internal" in tx_types
        assert len(rows) == 3

    @patch("sync_crypto.sync.EtherscanClient")
    @patch("sync_crypto.sync.ZerionClient")
    def test_sync_ethereum_zerion_native_eth_coexists_with_token_transfer(
        self, mock_zerion_cls, mock_eth_cls, tmp_path
    ):
        """A tx can have both a token transfer (Zerion) and a normal ETH transfer (Etherscan)."""
        from sync_crypto.sync import sync_wallet

        # Zerion returns a token transfer for the tx
        zerion_tx = TransactionRecord(
            record_id="zerion:tx-1:0",
            provider="zerion",
            network="ethereum",
            tx_hash="0xshared_hash",
            blockchain="ethereum",
            timestamp=1700000000,
            from_address="0xfrom",
            to_address="0xaddr",
            value="1000",
            fee="21000",
            status="success",
            tx_type="token_transfer",
            token_symbol="USDC",
        )
        mock_zerion = MagicMock()
        mock_zerion.fetch_new_transaction_batches.return_value = iter([[zerion_tx]])
        mock_zerion.advance_state.side_effect = [
            {"provider": "zerion"},
            {"provider": "zerion", "latest_timestamp": 1700000000, "latest_record_ids": ["zerion:tx-1:0"]},
        ]
        mock_zerion_cls.return_value = mock_zerion

        # Etherscan returns normal ETH for the same tx_hash — both should appear
        eth_tx = TransactionRecord(
            tx_hash="0xshared_hash",
            blockchain="ethereum",
            timestamp=1700000000,
            from_address="0xfrom",
            to_address="0xaddr",
            value="500000000000000000",
            fee="21000",
            status="success",
            tx_type="normal",
            currency="ETH",
        )
        mock_eth = MagicMock()
        mock_eth._fetch_normal_transactions.return_value = [eth_tx]
        mock_eth._fetch_internal_transactions.return_value = []
        mock_eth_cls.return_value = mock_eth

        wallet = WalletConfig(
            blockchain="ethereum",
            friendly_name="test_eth",
            address="0xaddr",
            provider="zerion",
            network="ethereum",
        )

        output = sync_wallet(
            wallet, tmp_path, etherscan_key="eth_key", zerion_api_key="zk_key"
        )

        rows = list(csv.DictReader(output.open()))
        assert len(rows) == 2  # Both token_transfer and normal
        types = {r["tx_type"] for r in rows}
        assert types == {"token_transfer", "normal"}

    @patch("sync_crypto.sync.EtherscanClient")
    @patch("sync_crypto.sync.ZerionClient")
    def test_sync_ethereum_zerion_native_eth_token_transfer_suppresses_etherscan_internal(
        self, mock_zerion_cls, mock_eth_cls, tmp_path
    ):
        """Zerion emits token_transfer rows for native ETH movements (e.g. Across
        bridge withdrawals) with an empty token_contract.  Etherscan's
        txlistinternal endpoint also surfaces the same on-chain ETH movement.
        The Etherscan ``internal`` row must be suppressed to prevent the wallet
        from being credited twice for one transfer."""
        from sync_crypto.sync import sync_wallet

        # Zerion's native ETH token_transfer (no token_contract) for the bridge tx
        zerion_native_eth = TransactionRecord(
            record_id="zerion:tx-1:0",
            provider="zerion",
            network="ethereum",
            tx_hash="0xbridge_tx",
            blockchain="ethereum",
            timestamp=1700000000,
            from_address="0xrelayer",
            to_address="0xaddr",
            value="3627342919737228329",
            fee="0",
            status="success",
            tx_type="token_transfer",
            token_symbol="ETH",
            token_contract=None,  # native ETH — no contract
        )
        mock_zerion = MagicMock()
        mock_zerion.fetch_new_transaction_batches.return_value = iter([[zerion_native_eth]])
        mock_zerion.advance_state.side_effect = [
            {"provider": "zerion"},
            {"provider": "zerion", "latest_timestamp": 1700000000, "latest_record_ids": ["zerion:tx-1:0"]},
        ]
        mock_zerion_cls.return_value = mock_zerion

        # Etherscan returns an internal ETH transfer for the SAME tx_hash — duplicate
        eth_internal_dup = TransactionRecord(
            tx_hash="0xbridge_tx",
            blockchain="ethereum",
            timestamp=1700000000,
            from_address="0xrelayer",
            to_address="0xaddr",
            value="3627342919737228329",
            fee="0",
            status="success",
            tx_type="internal",
            currency="ETH",
        )
        # And one for a different tx_hash that Zerion did NOT cover — should still be added
        eth_internal_unique = TransactionRecord(
            tx_hash="0xunique_internal",
            blockchain="ethereum",
            timestamp=1699999000,
            from_address="0xother",
            to_address="0xaddr",
            value="100000000000000000",
            fee="0",
            status="success",
            tx_type="internal",
            currency="ETH",
        )
        mock_eth = MagicMock()
        mock_eth._fetch_normal_transactions.return_value = []
        mock_eth._fetch_internal_transactions.return_value = [eth_internal_dup, eth_internal_unique]
        mock_eth_cls.return_value = mock_eth

        wallet = WalletConfig(
            blockchain="ethereum",
            friendly_name="test_eth",
            address="0xaddr",
            provider="zerion",
            network="ethereum",
        )

        output = sync_wallet(
            wallet, tmp_path, etherscan_key="eth_key", zerion_api_key="zk_key"
        )

        rows = list(csv.DictReader(output.open()))
        assert len(rows) == 2  # Zerion token_transfer + unique Etherscan internal (dup suppressed)
        hashes = {r["tx_hash"] for r in rows}
        assert hashes == {"0xbridge_tx", "0xunique_internal"}
        bridge_rows = [r for r in rows if r["tx_hash"] == "0xbridge_tx"]
        assert len(bridge_rows) == 1
        assert bridge_rows[0]["tx_type"] == "token_transfer"  # Zerion's row kept; Etherscan's internal dropped

    @patch("sync_crypto.sync.EtherscanClient")
    @patch("sync_crypto.sync.ZerionClient")
    def test_sync_ethereum_zerion_erc20_token_transfer_does_not_suppress_etherscan_internal(
        self, mock_zerion_cls, mock_eth_cls, tmp_path
    ):
        """Zerion ERC-20 token_transfer rows (with a populated token_contract)
        represent ERC-20 movements that have nothing to do with native ETH.
        They must NOT suppress Etherscan ``internal`` rows for the same tx_hash
        — those carry the separate native ETH leg of the same transaction
        (e.g. ETH refunds from a swap)."""
        from sync_crypto.sync import sync_wallet

        zerion_erc20 = TransactionRecord(
            record_id="zerion:tx-1:0",
            provider="zerion",
            network="ethereum",
            tx_hash="0xswap_tx",
            blockchain="ethereum",
            timestamp=1700000000,
            from_address="0xrouter",
            to_address="0xaddr",
            value="1000000",
            fee="0",
            status="success",
            tx_type="token_transfer",
            token_symbol="USDC",
            token_contract="0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",  # real USDC contract
        )
        mock_zerion = MagicMock()
        mock_zerion.fetch_new_transaction_batches.return_value = iter([[zerion_erc20]])
        mock_zerion.advance_state.side_effect = [
            {"provider": "zerion"},
            {"provider": "zerion", "latest_timestamp": 1700000000, "latest_record_ids": ["zerion:tx-1:0"]},
        ]
        mock_zerion_cls.return_value = mock_zerion

        eth_internal_refund = TransactionRecord(
            tx_hash="0xswap_tx",
            blockchain="ethereum",
            timestamp=1700000000,
            from_address="0xrouter",
            to_address="0xaddr",
            value="50000000000000000",
            fee="0",
            status="success",
            tx_type="internal",
            currency="ETH",
        )
        mock_eth = MagicMock()
        mock_eth._fetch_normal_transactions.return_value = []
        mock_eth._fetch_internal_transactions.return_value = [eth_internal_refund]
        mock_eth_cls.return_value = mock_eth

        wallet = WalletConfig(
            blockchain="ethereum",
            friendly_name="test_eth",
            address="0xaddr",
            provider="zerion",
            network="ethereum",
        )

        output = sync_wallet(
            wallet, tmp_path, etherscan_key="eth_key", zerion_api_key="zk_key"
        )

        rows = list(csv.DictReader(output.open()))
        assert len(rows) == 2  # ERC-20 token_transfer + ETH internal both retained
        types = {r["tx_type"] for r in rows}
        assert types == {"token_transfer", "internal"}

    @patch("sync_crypto.sync.EtherscanClient")
    @patch("sync_crypto.sync.ZerionClient")
    def test_sync_ethereum_native_eth_dedupe_matches_on_value_not_just_hash(
        self, mock_zerion_cls, mock_eth_cls, tmp_path
    ):
        """Some txs have multiple distinct internal ETH movements (e.g. main
        transfer + small refund) where Zerion only surfaces some of them.
        Etherscan internal rows whose value does NOT match any Zerion native
        ETH token_transfer for the same tx_hash must be retained."""
        from sync_crypto.sync import sync_wallet

        # Zerion captures the main ETH movement at the bridge layer (66 mwei)
        zerion_native_eth = TransactionRecord(
            record_id="zerion:tx-1:0",
            provider="zerion",
            network="ethereum",
            tx_hash="0xmulti_eth_tx",
            blockchain="ethereum",
            timestamp=1700000000,
            from_address="0xrelayer",
            to_address="0xaddr",
            value="66000000000000000",  # 0.066 ETH — matches one Etherscan internal row
            fee="0",
            status="success",
            tx_type="token_transfer",
            token_symbol="ETH",
            token_contract=None,
        )
        mock_zerion = MagicMock()
        mock_zerion.fetch_new_transaction_batches.return_value = iter([[zerion_native_eth]])
        mock_zerion.advance_state.side_effect = [
            {"provider": "zerion"},
            {"provider": "zerion", "latest_timestamp": 1700000000, "latest_record_ids": ["zerion:tx-1:0"]},
        ]
        mock_zerion_cls.return_value = mock_zerion

        # Etherscan returns TWO internal rows for the same tx — one matching
        # Zerion's value (the duplicate) and one with a different value (real
        # separate movement that Zerion missed; must be preserved).
        eth_internal_dup = TransactionRecord(
            tx_hash="0xmulti_eth_tx",
            blockchain="ethereum",
            timestamp=1700000000,
            from_address="0xrelayer",
            to_address="0xaddr",
            value="66000000000000000",
            fee="0",
            status="success",
            tx_type="internal",
            currency="ETH",
        )
        eth_internal_refund = TransactionRecord(
            tx_hash="0xmulti_eth_tx",
            blockchain="ethereum",
            timestamp=1700000000,
            from_address="0xrelayer",
            to_address="0xaddr",
            value="8800000000000",  # 0.0000088 ETH — different movement, must survive
            fee="0",
            status="success",
            tx_type="internal",
            currency="ETH",
        )
        mock_eth = MagicMock()
        mock_eth._fetch_normal_transactions.return_value = []
        mock_eth._fetch_internal_transactions.return_value = [eth_internal_dup, eth_internal_refund]
        mock_eth_cls.return_value = mock_eth

        wallet = WalletConfig(
            blockchain="ethereum",
            friendly_name="test_eth",
            address="0xaddr",
            provider="zerion",
            network="ethereum",
        )

        output = sync_wallet(
            wallet, tmp_path, etherscan_key="eth_key", zerion_api_key="zk_key"
        )

        rows = list(csv.DictReader(output.open()))
        assert len(rows) == 2  # Zerion + the unmatched-value internal refund
        internal_rows = [r for r in rows if r["tx_type"] == "internal"]
        assert len(internal_rows) == 1
        assert internal_rows[0]["value"] == "8800000000000"

    @patch("sync_crypto.sync.EtherscanClient")
    @patch("sync_crypto.sync.ZerionClient")
    def test_sync_ethereum_zerion_native_eth_token_transfer_suppresses_etherscan_normal(
        self, mock_zerion_cls, mock_eth_cls, tmp_path
    ):
        """Same bug class as the ``internal`` dedupe, but for Etherscan's
        ``txlist`` (``normal``) rows.  Two real captured pairs:

        - 0xe11708…0850b23: 2.0 ETH ShapeShift→wallet receive.  Zerion emits
          one ``token_transfer`` row (operation_type=receive, native ETH,
          empty token_contract); Etherscan ``txlist`` returns the same
          on-chain transfer as a ``normal`` row.
        - 0xad57cc…532396a: WETH deposit, wallet→WETH contract.  Zerion emits
          two transfers (incoming WETH with populated contract, outgoing ETH
          with empty contract); Etherscan ``txlist`` returns the outgoing
          ETH as a ``normal`` row.

        Both ``normal`` rows must be suppressed because a Zerion native-ETH
        ``token_transfer`` (empty ``token_contract``) for the same
        ``(tx_hash, value)`` already covers the on-chain movement.
        """
        from sync_crypto.sync import sync_wallet

        wallet_addr = "0xd2925983502b2f849c96dbe449179e8b09d8c6a7"

        # Sample 1: ShapeShift → wallet, 2 ETH receive
        zerion_receive_native_eth = TransactionRecord(
            record_id="zerion:0666c5f09d345cd98398a5e0d1f0ff03:0",
            provider="zerion",
            network="ethereum",
            tx_hash="0xe117086f8094e1a9934e5f16a367f95f5b340866068bd0cca11d3c3319850b23",
            blockchain="ethereum",
            timestamp=1532310775,
            from_address="0xeed16856d551569d134530ee3967ec79995e2051",
            to_address=wallet_addr,
            value="2000000000000000000",
            fee="1113000000000000",
            status="success",
            tx_type="token_transfer",
            token_name="Ethereum",
            token_symbol="ETH",
            token_contract=None,
            token_decimals=18,
            method="receive",
            currency="ETH",
        )

        # Sample 2: wallet → WETH, deposit() — Zerion emits TWO transfers
        zerion_deposit_in_weth = TransactionRecord(
            record_id="zerion:10fe662f86305fcc90e960d0fdbe57c4:0",
            provider="zerion",
            network="ethereum",
            tx_hash="0xad57ccb4e13eb5d2020e0a222371751847ff607e7371f730aa3247b43532396a",
            blockchain="ethereum",
            timestamp=1527111986,
            from_address="0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2",
            to_address=wallet_addr,
            value="2000000000000000000",
            fee="433460000000000",
            status="success",
            tx_type="token_transfer",
            token_name="Wrapped Ether",
            token_symbol="WETH",
            token_contract="0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2",
            token_decimals=18,
            method="deposit",
            currency="WETH",
        )
        zerion_deposit_out_native_eth = TransactionRecord(
            record_id="zerion:10fe662f86305fcc90e960d0fdbe57c4:1",
            provider="zerion",
            network="ethereum",
            tx_hash="0xad57ccb4e13eb5d2020e0a222371751847ff607e7371f730aa3247b43532396a",
            blockchain="ethereum",
            timestamp=1527111986,
            from_address=wallet_addr,
            to_address="0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2",
            value="2000000000000000000",
            fee="433460000000000",
            status="success",
            tx_type="token_transfer",
            token_name="Ethereum",
            token_symbol="ETH",
            token_contract=None,
            token_decimals=18,
            method="deposit",
            currency="ETH",
        )

        mock_zerion = MagicMock()
        mock_zerion.fetch_new_transaction_batches.return_value = iter([[
            zerion_receive_native_eth,
            zerion_deposit_in_weth,
            zerion_deposit_out_native_eth,
        ]])
        mock_zerion.advance_state.side_effect = [
            {"provider": "zerion"},
            {
                "provider": "zerion",
                "latest_timestamp": 1532310775,
                "latest_record_ids": ["zerion:0666c5f09d345cd98398a5e0d1f0ff03:0"],
            },
        ]
        mock_zerion_cls.return_value = mock_zerion

        # Etherscan returns the matching normal rows for both txs — both
        # must be suppressed by the Zerion native-ETH token_transfer rows.
        eth_normal_dup_receive = TransactionRecord(
            tx_hash="0xe117086f8094e1a9934e5f16a367f95f5b340866068bd0cca11d3c3319850b23",
            blockchain="ethereum",
            timestamp=1532310775,
            from_address="0xeed16856d551569d134530ee3967ec79995e2051",
            to_address=wallet_addr,
            value="2000000000000000000",
            fee="1113000000000000",
            status="success",
            tx_type="normal",
            currency="ETH",
            block_number=6013227,
            gas_used="21000",
            gas_price="53000000000",
        )
        eth_normal_dup_send = TransactionRecord(
            tx_hash="0xad57ccb4e13eb5d2020e0a222371751847ff607e7371f730aa3247b43532396a",
            blockchain="ethereum",
            timestamp=1527111986,
            from_address=wallet_addr,
            to_address="0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2",
            value="2000000000000000000",
            fee="433460000000000",
            status="success",
            tx_type="normal",
            currency="ETH",
            block_number=5665241,
            gas_used="43346",
            gas_price="10000000000",
            method_id="0xd0e30db0",
            function_name="deposit()",
        )
        mock_eth = MagicMock()
        mock_eth._fetch_normal_transactions.return_value = [
            eth_normal_dup_receive, eth_normal_dup_send,
        ]
        mock_eth._fetch_internal_transactions.return_value = []
        mock_eth_cls.return_value = mock_eth

        wallet = WalletConfig(
            blockchain="ethereum",
            friendly_name="test_eth",
            address=wallet_addr,
            provider="zerion",
            network="ethereum",
        )

        output = sync_wallet(
            wallet, tmp_path, etherscan_key="eth_key", zerion_api_key="zk_key"
        )

        rows = list(csv.DictReader(output.open()))
        # Expected: 3 Zerion token_transfer rows, 0 Etherscan normal rows
        assert len(rows) == 3, [(r["tx_type"], r["currency"], r["value"]) for r in rows]
        types = [r["tx_type"] for r in rows]
        assert types.count("token_transfer") == 3
        assert types.count("normal") == 0

    @patch("sync_crypto.sync.EtherscanClient")
    @patch("sync_crypto.sync.ZerionClient")
    def test_sync_ethereum_zerion_erc20_token_transfer_does_not_suppress_etherscan_normal(
        self, mock_zerion_cls, mock_eth_cls, tmp_path
    ):
        """An ERC-20 ``token_transfer`` (populated ``token_contract``) must
        NOT suppress the Etherscan ``normal`` row for the same tx_hash even
        when the values happen to match — they describe distinct on-chain
        movements (a token transfer + a separate native ETH leg, e.g. a
        contract call that both moves an ERC-20 and forwards ETH)."""
        from sync_crypto.sync import sync_wallet

        zerion_erc20 = TransactionRecord(
            record_id="zerion:tx-1:0",
            provider="zerion",
            network="ethereum",
            tx_hash="0xshared_hash",
            blockchain="ethereum",
            timestamp=1700000000,
            from_address="0xrouter",
            to_address="0xaddr",
            value="500000000000000000",
            fee="0",
            status="success",
            tx_type="token_transfer",
            token_symbol="USDC",
            token_contract="0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
        )
        mock_zerion = MagicMock()
        mock_zerion.fetch_new_transaction_batches.return_value = iter([[zerion_erc20]])
        mock_zerion.advance_state.side_effect = [
            {"provider": "zerion"},
            {"provider": "zerion", "latest_timestamp": 1700000000, "latest_record_ids": ["zerion:tx-1:0"]},
        ]
        mock_zerion_cls.return_value = mock_zerion

        eth_normal = TransactionRecord(
            tx_hash="0xshared_hash",
            blockchain="ethereum",
            timestamp=1700000000,
            from_address="0xsender",
            to_address="0xaddr",
            value="500000000000000000",
            fee="21000",
            status="success",
            tx_type="normal",
            currency="ETH",
        )
        mock_eth = MagicMock()
        mock_eth._fetch_normal_transactions.return_value = [eth_normal]
        mock_eth._fetch_internal_transactions.return_value = []
        mock_eth_cls.return_value = mock_eth

        wallet = WalletConfig(
            blockchain="ethereum",
            friendly_name="test_eth",
            address="0xaddr",
            provider="zerion",
            network="ethereum",
        )

        output = sync_wallet(
            wallet, tmp_path, etherscan_key="eth_key", zerion_api_key="zk_key"
        )

        rows = list(csv.DictReader(output.open()))
        assert len(rows) == 2  # ERC-20 token_transfer + normal native ETH both retained
        types = {r["tx_type"] for r in rows}
        assert types == {"token_transfer", "normal"}

    def test_sync_zerion_wallet_requires_api_key(self, tmp_path):
        from sync_crypto.sync import sync_wallet

        wallet = WalletConfig(
            blockchain="ethereum",
            friendly_name="test_eth",
            address="0xaddr",
            provider="zerion",
        )

        assert sync_wallet(wallet, tmp_path, zerion_api_key=None) is None

    @patch("sync_crypto.sync.SolanaRpcClient")
    @patch("sync_crypto.sync.ZerionClient")
    def test_sync_solana_zerion_wallet_passes_solana_rpc_client(
        self,
        mock_zerion_cls,
        mock_solana_rpc_cls,
        tmp_path,
    ):
        from sync_crypto.sync import sync_wallet

        mock_client = MagicMock()
        mock_client.fetch_new_transaction_batches.return_value = iter([])
        mock_client.advance_state.return_value = {"provider": "zerion"}
        mock_zerion_cls.return_value = mock_client
        mock_solana_rpc = MagicMock()
        mock_solana_rpc_cls.return_value = mock_solana_rpc

        wallet = WalletConfig(
            blockchain="solana",
            friendly_name="test_sol",
            address="So11111111111111111111111111111111111111112",
            provider="zerion",
        )

        sync_wallet(wallet, tmp_path, zerion_api_key="zk_dev_test", solana_rpc_url="https://rpc.example")

        mock_solana_rpc_cls.assert_called_once_with(rpc_url="https://rpc.example")
        assert mock_zerion_cls.call_args.kwargs["solana_rpc_client"] is mock_solana_rpc


class TestSyncAll:
    @patch("sync_crypto.sync.sync_source")
    def test_sync_all_processes_each_source(self, mock_sync_source, tmp_path):
        from sync_crypto.sync import sync_all

        mock_sync_source.return_value = tmp_path / "dummy.csv"

        sources = [
            WalletConfig(blockchain="ethereum", friendly_name="eth1", address="0xa"),
            WalletConfig(blockchain="solana", friendly_name="sol1", address="7xK"),
        ]
        results = sync_all(sources, tmp_path, etherscan_key="ek")
        assert len(results) == 2
        assert mock_sync_source.call_count == 2

    @patch("sync_crypto.sync.sync_source")
    def test_sync_all_continues_on_failure(self, mock_sync_source, tmp_path):
        from sync_crypto.sync import sync_all

        mock_sync_source.side_effect = [None, tmp_path / "ok.csv"]

        sources = [
            WalletConfig(blockchain="ethereum", friendly_name="fail", address="0xa"),
            WalletConfig(blockchain="solana", friendly_name="ok", address="7xK"),
        ]
        results = sync_all(sources, tmp_path, etherscan_key="ek")
        assert len(results) == 2
        assert results[0] is None
        assert results[1] is not None


class TestSyncSource:
    @patch("sync_crypto.sync.sync_binance_account")
    def test_sync_source_dispatches_binance(self, mock_sync_binance, tmp_path):
        from sync_crypto.sync import sync_source

        mock_sync_binance.return_value = tmp_path / "binance.csv"

        source = BinanceConfig(
            exchange="binance",
            friendly_name="main_binance",
            symbols=["BTCUSDT"],
        )

        result = sync_source(
            source,
            tmp_path,
            binance_api_key="key",
            binance_api_secret="secret",
            binance_max_rows=25,
        )

        assert result == tmp_path / "binance.csv"
        mock_sync_binance.assert_called_once_with(
            source,
            tmp_path,
            api_key="key",
            api_secret="secret",
            max_rows=25,
        )

    def test_sync_source_requires_binance_credentials(self, tmp_path):
        from sync_crypto.sync import sync_source

        source = BinanceConfig(
            exchange="binance",
            friendly_name="main_binance",
            symbols=["BTCUSDT"],
        )

        result = sync_source(source, tmp_path, binance_api_key=None, binance_api_secret=None)

        assert result is None


def _write_test_csv(path: Path, rows: list[dict]):
    """Helper to write a CSV with CSV_FIELDS header and given rows."""
    with open(path, "w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=CSV_FIELDS)
        writer.writeheader()
        for row in rows:
            full_row = {k: "" for k in CSV_FIELDS}
            full_row.update(row)
            writer.writerow(full_row)


class TestReadLastCursor:
    def test_returns_cursor_from_existing_csv(self, tmp_path):
        from sync_crypto.sync import read_last_cursor

        csv_path = tmp_path / "test_transactions.csv"
        _write_test_csv(csv_path, [
            {"tx_hash": "0xfirst", "timestamp": "1700000000", "block_number": "100"},
            {"tx_hash": "0xlast", "timestamp": "1700000100", "block_number": "200"},
        ])
        cursor = read_last_cursor(csv_path)
        assert cursor["tx_hash"] == "0xlast"
        assert cursor["block_number"] == 200

    def test_returns_newest_by_timestamp_not_row_order(self, tmp_path):
        from sync_crypto.sync import read_last_cursor

        csv_path = tmp_path / "test_transactions.csv"
        # Newest timestamp first (like Solana chunked writes)
        _write_test_csv(csv_path, [
            {"tx_hash": "5newest", "timestamp": "1700000200", "block_number": "300"},
            {"tx_hash": "5middle", "timestamp": "1700000100", "block_number": "200"},
            {"tx_hash": "5oldest", "timestamp": "1700000000", "block_number": "100"},
        ])
        cursor = read_last_cursor(csv_path)
        assert cursor["tx_hash"] == "5newest"
        assert cursor["block_number"] == 300

    def test_returns_empty_when_no_file(self, tmp_path):
        from sync_crypto.sync import read_last_cursor

        csv_path = tmp_path / "nonexistent.csv"
        cursor = read_last_cursor(csv_path)
        assert cursor == {}

    def test_returns_empty_for_header_only_csv(self, tmp_path):
        from sync_crypto.sync import read_last_cursor

        csv_path = tmp_path / "empty_transactions.csv"
        _write_test_csv(csv_path, [])
        cursor = read_last_cursor(csv_path)
        assert cursor == {}

    def test_returns_empty_block_number_when_missing(self, tmp_path):
        from sync_crypto.sync import read_last_cursor

        csv_path = tmp_path / "sol_transactions.csv"
        _write_test_csv(csv_path, [
            {"tx_hash": "5solsig", "timestamp": "1700000000", "block_number": ""},
        ])
        cursor = read_last_cursor(csv_path)
        assert cursor["tx_hash"] == "5solsig"
        assert cursor["block_number"] is None


class TestAppendCsv:
    def test_write_csv_append_adds_rows_without_header(self, tmp_path):
        from sync_crypto.sync import write_csv

        # Write initial CSV
        existing = [
            TransactionRecord(
                tx_hash="0xfirst", blockchain="ethereum", timestamp=1700000000,
                from_address="0xfrom", to_address="0xto", value="1000",
                fee="21000", status="success", block_number=100,
            ),
        ]
        write_csv(existing, "ethereum", "0xappend", tmp_path)

        # Append new transactions
        new_txs = [
            TransactionRecord(
                tx_hash="0xsecond", blockchain="ethereum", timestamp=1700000100,
                from_address="0xfrom", to_address="0xto", value="2000",
                fee="21000", status="success", block_number=200,
            ),
        ]
        output = write_csv(new_txs, "ethereum", "0xappend", tmp_path, append=True)

        content = output.read_text()
        lines = content.strip().split("\n")
        assert len(lines) == 3  # 1 header + 2 rows
        assert lines[0].startswith("record_id,provider,network,tx_hash")  # only one header
        assert "0xfirst" in lines[1]
        assert "0xsecond" in lines[2]

    def test_write_csv_append_deduplicates_by_tx_hash(self, tmp_path):
        from sync_crypto.sync import write_csv

        existing = [
            TransactionRecord(
                tx_hash="0xdup", blockchain="ethereum", timestamp=1700000000,
                from_address="0xfrom", to_address="0xto", value="1000",
                fee="21000", status="success", block_number=100,
            ),
        ]
        write_csv(existing, "ethereum", "0xdedupe", tmp_path)

        # Append with a duplicate
        new_txs = [
            TransactionRecord(
                tx_hash="0xdup", blockchain="ethereum", timestamp=1700000000,
                from_address="0xfrom", to_address="0xto", value="1000",
                fee="21000", status="success", block_number=100,
            ),
            TransactionRecord(
                tx_hash="0xnew", blockchain="ethereum", timestamp=1700000100,
                from_address="0xfrom", to_address="0xto", value="2000",
                fee="21000", status="success", block_number=200,
            ),
        ]
        output = write_csv(new_txs, "ethereum", "0xdedupe", tmp_path, append=True)

        content = output.read_text()
        lines = content.strip().split("\n")
        assert len(lines) == 3  # header + original + new (no dup)
        assert content.count("0xdup") == 1

    def test_write_csv_chunked_append(self, tmp_path):
        from sync_crypto.sync import _write_csv_chunked

        # Write initial CSV
        csv_path = tmp_path / "solana_SolAddr_transactions.csv"
        _write_test_csv(csv_path, [
            {"tx_hash": "5first", "blockchain": "solana", "timestamp": "1700000000"},
        ])

        # Append chunks
        new_chunks = [[
            TransactionRecord(
                tx_hash="5second", blockchain="solana", timestamp=1700000100,
                from_address="7xK", to_address="8yL", value="1000",
                fee="5000", status="success",
            ),
        ]]
        output = _write_csv_chunked(
            iter(new_chunks), "solana", "SolAddr", tmp_path, append=True,
        )

        content = output.read_text()
        lines = content.strip().split("\n")
        assert len(lines) == 3  # header + 2 rows
        assert "5first" in lines[1]
        assert "5second" in lines[2]

    def test_write_csv_chunked_append_deduplicates(self, tmp_path):
        from sync_crypto.sync import _write_csv_chunked

        csv_path = tmp_path / "solana_SolAddr_transactions.csv"
        _write_test_csv(csv_path, [
            {"tx_hash": "5dup", "blockchain": "solana", "timestamp": "1700000000"},
        ])

        new_chunks = [[
            TransactionRecord(
                tx_hash="5dup", blockchain="solana", timestamp=1700000000,
                from_address="7xK", to_address="8yL", value="1000",
                fee="5000", status="success",
            ),
            TransactionRecord(
                tx_hash="5new", blockchain="solana", timestamp=1700000100,
                from_address="7xK", to_address="8yL", value="2000",
                fee="5000", status="success",
            ),
        ]]
        output = _write_csv_chunked(
            iter(new_chunks), "solana", "SolAddr", tmp_path, append=True,
        )

        content = output.read_text()
        assert content.count("5dup") == 1
        assert "5new" in content

    def test_write_csv_append_migrates_legacy_header(self, tmp_path):
        from sync_crypto.sync import write_csv

        csv_path = tmp_path / "ethereum_0xnewwallet_transactions.csv"
        legacy_fields = [
            "tx_hash", "blockchain", "timestamp", "from_address", "to_address",
            "value", "fee", "status", "tx_type", "token_name", "token_symbol",
            "token_contract", "token_decimals", "block_number", "gas_used",
            "gas_price", "method",
        ]
        with open(csv_path, "w", newline="") as f:
            writer = csv.DictWriter(f, fieldnames=legacy_fields)
            writer.writeheader()
            writer.writerow({
                "tx_hash": "0xlegacy",
                "blockchain": "ethereum",
                "timestamp": "1700000000",
                "from_address": "0xfrom",
                "to_address": "0xto",
                "value": "1",
                "fee": "1",
                "status": "success",
                "tx_type": "normal",
                "block_number": "100",
            })

        new_txs = [
            TransactionRecord(
                tx_hash="0xnew",
                blockchain="ethereum",
                timestamp=1700000100,
                from_address="0xfrom",
                to_address="0xto",
                value="2",
                fee="2",
                status="success",
                tx_type="normal",
                currency="ETH",
            ),
        ]
        output = write_csv(new_txs, "ethereum", "0xnewwallet", tmp_path, append=True)

        with open(output, newline="") as f:
            reader = csv.DictReader(f)
            assert reader.fieldnames == CSV_FIELDS
            rows = list(reader)

        assert len(rows) == 2
        legacy_row = next(r for r in rows if r["tx_hash"] == "0xlegacy")
        new_row = next(r for r in rows if r["tx_hash"] == "0xnew")
        assert legacy_row["currency"] == ""
        assert new_row["currency"] == "ETH"

    def test_write_csv_chunked_append_migrates_legacy_header(self, tmp_path):
        from sync_crypto.sync import _write_csv_chunked

        csv_path = tmp_path / "solana_SolWallet_transactions.csv"
        legacy_fields = [
            "tx_hash", "blockchain", "timestamp", "from_address", "to_address",
            "value", "fee", "status", "tx_type", "token_name", "token_symbol",
            "token_contract", "token_decimals", "block_number", "gas_used",
            "gas_price", "method",
        ]
        with open(csv_path, "w", newline="") as f:
            writer = csv.DictWriter(f, fieldnames=legacy_fields)
            writer.writeheader()
            writer.writerow({
                "tx_hash": "5legacy",
                "blockchain": "solana",
                "timestamp": "1700000000",
                "from_address": "7xK",
                "to_address": "8yL",
                "value": "1",
                "fee": "5000",
                "status": "success",
                "tx_type": "native",
            })

        new_chunks = [[
            TransactionRecord(
                tx_hash="5new",
                blockchain="solana",
                timestamp=1700000100,
                from_address="7xK",
                to_address="8yL",
                value="2",
                fee="5000",
                status="success",
                tx_type="native",
                currency="SOL",
            ),
        ]]
        output = _write_csv_chunked(
            iter(new_chunks), "solana", "SolWallet", tmp_path, append=True,
        )

        with open(output, newline="") as f:
            reader = csv.DictReader(f)
            assert reader.fieldnames == CSV_FIELDS
            rows = list(reader)

        assert len(rows) == 2
        legacy_row = next(r for r in rows if r["tx_hash"] == "5legacy")
        new_row = next(r for r in rows if r["tx_hash"] == "5new")
        assert legacy_row["currency"] == ""
        assert new_row["currency"] == "SOL"
