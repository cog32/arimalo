"""Tests for sync_crypto.models."""
import pytest
from pydantic import ValidationError


class TestBlockchain:
    def test_ethereum_value(self):
        from sync_crypto.models import Blockchain
        assert Blockchain.ETHEREUM == "ethereum"

    def test_solana_value(self):
        from sync_crypto.models import Blockchain
        assert Blockchain.SOLANA == "solana"


class TestWalletConfig:
    def test_valid_ethereum_wallet(self):
        from sync_crypto.models import WalletConfig
        wallet = WalletConfig(
            blockchain="ethereum",
            friendly_name="main_eth",
            address="0xde0B295669a9FD93d5F28D9Ec85E40f4cb697BAe",
        )
        assert wallet.blockchain == "ethereum"
        assert wallet.friendly_name == "main_eth"
        assert wallet.address == "0xde0B295669a9FD93d5F28D9Ec85E40f4cb697BAe"

    def test_valid_solana_wallet(self):
        from sync_crypto.models import WalletConfig
        wallet = WalletConfig(
            blockchain="solana",
            friendly_name="sol_defi",
            address="7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
        )
        assert wallet.blockchain == "solana"
        assert wallet.friendly_name == "sol_defi"

    def test_invalid_blockchain_rejected(self):
        from sync_crypto.models import WalletConfig
        with pytest.raises(ValidationError):
            WalletConfig(
                blockchain="bitcoin",
                friendly_name="btc_main",
                address="1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa",
            )

    def test_empty_address_rejected(self):
        from sync_crypto.models import WalletConfig
        with pytest.raises(ValidationError):
            WalletConfig(
                blockchain="ethereum",
                friendly_name="empty",
                address="",
            )

    def test_empty_friendly_name_rejected(self):
        from sync_crypto.models import WalletConfig
        with pytest.raises(ValidationError):
            WalletConfig(
                blockchain="ethereum",
                friendly_name="",
                address="0xabc123",
            )

    def test_provider_and_network_are_optional(self):
        from sync_crypto.models import WalletConfig

        wallet = WalletConfig(
            blockchain="ethereum",
            friendly_name="main_eth",
            address="0xabc123",
            provider="zerion",
            network=" Base ",
        )

        assert wallet.provider == "zerion"
        assert wallet.network == "base"


class TestBinanceConfig:
    def test_valid_binance_config_normalizes_symbols(self):
        from sync_crypto.models import BinanceConfig, DEFAULT_BINANCE_TRANSFER_TYPES

        config = BinanceConfig(
            exchange="binance",
            friendly_name="main_binance",
            symbols=["btcusdt", "BTCUSDT", " ethusdt "],
        )

        assert config.exchange == "binance"
        assert config.friendly_name == "main_binance"
        assert config.symbols == ["BTCUSDT", "ETHUSDT"]
        assert config.transfer_types == DEFAULT_BINANCE_TRANSFER_TYPES

    def test_binance_config_rejects_empty_symbols(self):
        from sync_crypto.models import BinanceConfig

        with pytest.raises(ValidationError):
            BinanceConfig(
                exchange="binance",
                friendly_name="main_binance",
                symbols=[],
            )

    def test_binance_config_rejects_invalid_exchange(self):
        from sync_crypto.models import BinanceConfig

        with pytest.raises(ValidationError):
            BinanceConfig(
                exchange="kraken",
                friendly_name="main_binance",
                symbols=["BTCUSDT"],
            )


class TestTransactionRecord:
    def test_minimal_transaction(self):
        from sync_crypto.models import TransactionRecord
        tx = TransactionRecord(
            tx_hash="0xabc123",
            blockchain="ethereum",
            timestamp=1700000000,
            from_address="0xfrom",
            to_address="0xto",
            value="1000000000000000000",
            fee="21000000000000",
            status="success",
        )
        assert tx.record_id is None
        assert tx.provider is None
        assert tx.network is None
        assert tx.tx_hash == "0xabc123"
        assert tx.blockchain == "ethereum"
        assert tx.timestamp == 1700000000
        assert tx.value == "1000000000000000000"
        assert tx.fee == "21000000000000"
        assert tx.status == "success"

    def test_full_transaction_with_token_info(self):
        from sync_crypto.models import TransactionRecord
        tx = TransactionRecord(
            tx_hash="0xdef456",
            blockchain="ethereum",
            timestamp=1700000000,
            from_address="0xfrom",
            to_address="0xto",
            value="500000000",
            fee="42000000000000",
            status="success",
            tx_type="token_transfer",
            token_name="USD Coin",
            token_symbol="USDC",
            token_contract="0xa0b8699",
            token_decimals=6,
            block_number=18500000,
            gas_used="21000",
            gas_price="20000000000",
            method="transfer",
            currency="USDC",
            method_id="0xa9059cbb",
            function_name="transfer(address,uint256)",
            input_data="0xabcdef",
            tx_receipt_status="1",
            transaction_index=7,
            cumulative_gas_used="42000",
            confirmations=120,
        )
        assert tx.token_name == "USD Coin"
        assert tx.token_symbol == "USDC"
        assert tx.token_decimals == 6
        assert tx.block_number == 18500000
        assert tx.gas_used == "21000"
        assert tx.method == "transfer"
        assert tx.currency == "USDC"
        assert tx.method_id == "0xa9059cbb"
        assert tx.function_name == "transfer(address,uint256)"
        assert tx.input_data == "0xabcdef"
        assert tx.tx_receipt_status == "1"
        assert tx.transaction_index == 7
        assert tx.cumulative_gas_used == "42000"
        assert tx.confirmations == 120

    def test_optional_fields_default_none(self):
        from sync_crypto.models import TransactionRecord
        tx = TransactionRecord(
            tx_hash="0x123",
            blockchain="solana",
            timestamp=1700000000,
            from_address="7xK",
            to_address="8yL",
            value="1000000000",
            fee="5000",
            status="success",
        )
        assert tx.tx_type is None
        assert tx.token_name is None
        assert tx.token_symbol is None
        assert tx.token_contract is None
        assert tx.token_decimals is None
        assert tx.block_number is None
        assert tx.gas_used is None
        assert tx.gas_price is None
        assert tx.method is None
        assert tx.currency is None
        assert tx.method_id is None
        assert tx.function_name is None
        assert tx.input_data is None
        assert tx.tx_receipt_status is None
        assert tx.transaction_index is None
        assert tx.cumulative_gas_used is None
        assert tx.confirmations is None

    def test_value_stored_as_string(self):
        """Values must be strings to avoid precision loss with wei/lamport amounts."""
        from sync_crypto.models import TransactionRecord
        tx = TransactionRecord(
            tx_hash="0x123",
            blockchain="ethereum",
            timestamp=1700000000,
            from_address="0xfrom",
            to_address="0xto",
            value="999999999999999999999",
            fee="21000000000000",
            status="success",
        )
        assert isinstance(tx.value, str)
        assert tx.value == "999999999999999999999"

    def test_to_csv_row(self):
        from sync_crypto.models import TransactionRecord
        tx = TransactionRecord(
            record_id="zerion:tx:1",
            provider="zerion",
            network="ethereum",
            tx_hash="0xabc",
            blockchain="ethereum",
            timestamp=1700000000,
            from_address="0xfrom",
            to_address="0xto",
            value="1000",
            fee="21000",
            status="success",
            block_number=18500000,
        )
        row = tx.to_csv_row()
        assert row["record_id"] == "zerion:tx:1"
        assert row["provider"] == "zerion"
        assert row["network"] == "ethereum"
        assert row["tx_hash"] == "0xabc"
        assert row["blockchain"] == "ethereum"
        assert row["block_number"] == 18500000
        assert row["token_name"] is None
        assert row["currency"] is None
        assert row["method_id"] is None
