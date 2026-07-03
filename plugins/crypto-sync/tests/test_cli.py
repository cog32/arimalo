"""Tests for sync_crypto.cli."""
from unittest.mock import patch

import pytest


class TestParseArgs:
    def test_default_args(self):
        from sync_crypto.cli import parse_args

        args = parse_args(["wallets.json"])
        assert args.config == "wallets.json"
        assert args.output_dir == "output"

    def test_custom_output_dir(self):
        from sync_crypto.cli import parse_args

        args = parse_args(["wallets.json", "-o", "/tmp/crypto"])
        assert args.output_dir == "/tmp/crypto"

    def test_missing_config_raises(self):
        from sync_crypto.cli import parse_args

        with pytest.raises(SystemExit):
            parse_args([])

    def test_binance_max_rows_arg(self):
        from sync_crypto.cli import parse_args

        args = parse_args(["wallets.json", "--binance-max-rows", "25"])
        assert args.binance_max_rows == 25

    def test_binance_max_rows_rejects_zero(self):
        from sync_crypto.cli import parse_args

        with pytest.raises(SystemExit):
            parse_args(["wallets.json", "--binance-max-rows", "0"])

    def test_cache_flags_default_off(self):
        from sync_crypto.cli import parse_args

        args = parse_args(["wallets.json"])
        assert args.update_cache is False
        assert args.from_cache is False
        assert args.cache_dir is None

    def test_update_cache_flag(self):
        from sync_crypto.cli import parse_args

        args = parse_args(["wallets.json", "--update-cache"])
        assert args.update_cache is True
        assert args.from_cache is False

    def test_from_cache_flag(self):
        from sync_crypto.cli import parse_args

        args = parse_args(["wallets.json", "--from-cache"])
        assert args.from_cache is True
        assert args.update_cache is False

    def test_update_cache_and_from_cache_mutually_exclusive(self):
        from sync_crypto.cli import parse_args

        with pytest.raises(SystemExit):
            parse_args(["wallets.json", "--update-cache", "--from-cache"])

    def test_cache_dir_override(self):
        from sync_crypto.cli import parse_args

        args = parse_args(["wallets.json", "--cache-dir", "/tmp/cache"])
        assert args.cache_dir == "/tmp/cache"


class TestResolveCacheMode:
    def test_default_is_off(self):
        from sync_crypto.cli import _resolve_cache_mode, parse_args
        from sync_crypto.cache_orchestration import CacheMode

        assert _resolve_cache_mode(parse_args(["wallets.json"])) == CacheMode.OFF

    def test_from_cache_is_read(self):
        from sync_crypto.cli import _resolve_cache_mode, parse_args
        from sync_crypto.cache_orchestration import CacheMode

        assert _resolve_cache_mode(parse_args(["wallets.json", "--from-cache"])) == CacheMode.READ

    def test_update_cache_is_write(self):
        from sync_crypto.cli import _resolve_cache_mode, parse_args
        from sync_crypto.cache_orchestration import CacheMode

        assert _resolve_cache_mode(parse_args(["wallets.json", "--update-cache"])) == CacheMode.WRITE


class TestResolveCacheDir:
    def test_none_when_no_cache_flag(self, tmp_path):
        from sync_crypto.cli import _resolve_cache_dir, parse_args

        assert _resolve_cache_dir(parse_args(["wallets.json"]), tmp_path) is None

    def test_default_is_output_dir_dot_cache(self, tmp_path):
        from sync_crypto.cli import _resolve_cache_dir, parse_args

        result = _resolve_cache_dir(
            parse_args(["wallets.json", "--update-cache"]), tmp_path,
        )
        assert result == tmp_path / ".cache"

    def test_explicit_override(self, tmp_path):
        from pathlib import Path
        from sync_crypto.cli import _resolve_cache_dir, parse_args

        result = _resolve_cache_dir(
            parse_args(["wallets.json", "--from-cache", "--cache-dir", "/tmp/x"]),
            tmp_path,
        )
        assert result == Path("/tmp/x")


class TestMain:
    @patch("sync_crypto.cli.sync_all")
    @patch("sync_crypto.cli.load_config")
    @patch("sync_crypto.cli.load_dotenv")
    def test_main_success(self, mock_dotenv, mock_load, mock_sync, tmp_path, monkeypatch):
        from sync_crypto.cli import main
        from sync_crypto.models import WalletConfig

        # Ensure only ETHERSCAN_API_KEY is set; clear any leaked from .env
        monkeypatch.setenv("ETHERSCAN_API_KEY", "eth_key")
        for var in ("SOLANA_RPC_URL", "ZERION_API_KEY", "BINANCE_API_KEY", "BINANCE_API_SECRET"):
            monkeypatch.delenv(var, raising=False)

        config_file = tmp_path / "wallets.json"
        config_file.write_text("[]")
        output_dir = tmp_path / "output"

        mock_load.return_value = [
            WalletConfig(blockchain="ethereum", friendly_name="eth1", address="0xa"),
        ]
        mock_sync.return_value = [output_dir / "ethereum_0xa_transactions.csv"]

        main([str(config_file), "-o", str(output_dir)])

        mock_dotenv.assert_called_once()
        mock_load.assert_called_once()
        mock_sync.assert_called_once()

        # Verify API key was passed (no solscan_key needed)
        call_kwargs = mock_sync.call_args[1]
        assert call_kwargs["etherscan_key"] == "eth_key"
        assert call_kwargs["solana_rpc_url"] is None
        assert call_kwargs["zerion_api_key"] is None
        assert call_kwargs["binance_api_key"] is None
        assert call_kwargs["binance_api_secret"] is None

    @patch("sync_crypto.cli.sync_all")
    @patch("sync_crypto.cli.load_config")
    @patch("sync_crypto.cli.load_dotenv")
    @patch.dict("os.environ", {"HELIUS_API_KEY": "helius_test_key"}, clear=True)
    def test_main_builds_solana_rpc_url_from_helius_api_key(
        self,
        mock_dotenv,
        mock_load,
        mock_sync,
        tmp_path,
    ):
        from sync_crypto.cli import main
        from sync_crypto.models import WalletConfig

        config_file = tmp_path / "wallets.json"
        config_file.write_text("[]")

        mock_load.return_value = [
            WalletConfig(blockchain="solana", friendly_name="sol1", address="wallet1"),
        ]
        mock_sync.return_value = [tmp_path / "output" / "solana_wallet1_transactions.csv"]

        main([str(config_file), "-o", str(tmp_path / "output")])

        call_kwargs = mock_sync.call_args[1]
        assert (
            call_kwargs["solana_rpc_url"]
            == "https://mainnet.helius-rpc.com/?api-key=helius_test_key"
        )

    @patch("sync_crypto.cli.sync_all")
    @patch("sync_crypto.cli.load_config")
    @patch("sync_crypto.cli.load_dotenv")
    @patch.dict(
        "os.environ",
        {
            "SOLANA_RPC_URL": "https://env-rpc.test",
            "HELIUS_API_KEY": "helius_test_key",
        },
        clear=True,
    )
    def test_main_prefers_helius_api_key_over_solana_rpc_url(
        self,
        mock_dotenv,
        mock_load,
        mock_sync,
        tmp_path,
    ):
        from sync_crypto.cli import main
        from sync_crypto.models import WalletConfig

        config_file = tmp_path / "wallets.json"
        config_file.write_text("[]")

        mock_load.return_value = [
            WalletConfig(blockchain="solana", friendly_name="sol1", address="wallet1"),
        ]
        mock_sync.return_value = [tmp_path / "output" / "solana_wallet1_transactions.csv"]

        main([str(config_file), "-o", str(tmp_path / "output")])

        call_kwargs = mock_sync.call_args[1]
        assert (
            call_kwargs["solana_rpc_url"]
            == "https://mainnet.helius-rpc.com/?api-key=helius_test_key"
        )

    @patch("sync_crypto.cli.load_dotenv")
    def test_main_missing_config_file(self, mock_dotenv, tmp_path, capsys):
        from sync_crypto.cli import main

        with pytest.raises(SystemExit) as exc_info:
            main([str(tmp_path / "nonexistent.json")])
        assert exc_info.value.code == 1

    @patch("sync_crypto.cli.sync_all")
    @patch("sync_crypto.cli.load_config")
    @patch("sync_crypto.cli.load_dotenv")
    @patch.dict("os.environ", {}, clear=True)
    def test_main_missing_api_keys_warns(self, mock_dotenv, mock_load, mock_sync, tmp_path, capsys):
        from sync_crypto.cli import main
        from sync_crypto.models import WalletConfig

        config_file = tmp_path / "wallets.json"
        config_file.write_text("[]")

        mock_load.return_value = [
            WalletConfig(blockchain="ethereum", friendly_name="eth1", address="0xa"),
        ]
        mock_sync.return_value = [None]

        main([str(config_file), "-o", str(tmp_path / "output")])

        # Should still proceed (key is None)
        call_kwargs = mock_sync.call_args[1]
        assert call_kwargs["etherscan_key"] is None
        assert call_kwargs["solana_rpc_url"] is None
        assert call_kwargs["zerion_api_key"] is None
        assert call_kwargs["binance_api_key"] is None
        assert call_kwargs["binance_api_secret"] is None

    @patch("sync_crypto.cli.sync_all")
    @patch("sync_crypto.cli.load_config")
    @patch("sync_crypto.cli.load_dotenv")
    @patch.dict(
        "os.environ",
        {
            "BINANCE_API_KEY": "binance_key",
            "BINANCE_API_SECRET": "binance_secret",
            "BINANCE_MAX_ROWS": "7",
        },
        clear=True,
    )
    def test_main_reads_binance_max_rows_from_env(self, mock_dotenv, mock_load, mock_sync, tmp_path):
        from sync_crypto.cli import main
        from sync_crypto.models import BinanceConfig

        config_file = tmp_path / "wallets.json"
        config_file.write_text("[]")

        mock_load.return_value = [
            BinanceConfig(exchange="binance", friendly_name="main_binance", symbols=["BTCUSDT"]),
        ]
        mock_sync.return_value = [tmp_path / "output" / "binance_main_binance_accounting.csv"]

        main([str(config_file), "-o", str(tmp_path / "output")])

        call_kwargs = mock_sync.call_args[1]
        assert call_kwargs["binance_api_key"] == "binance_key"
        assert call_kwargs["binance_api_secret"] == "binance_secret"
        assert call_kwargs["binance_max_rows"] == 7

    @patch("sync_crypto.cli.sync_all")
    @patch("sync_crypto.cli.load_config")
    @patch("sync_crypto.cli.load_dotenv")
    @patch.dict("os.environ", {"ZERION_API_KEY": "zk_dev_test"}, clear=True)
    def test_main_passes_zerion_api_key(self, mock_dotenv, mock_load, mock_sync, tmp_path):
        from sync_crypto.cli import main
        from sync_crypto.models import WalletConfig

        config_file = tmp_path / "wallets.json"
        config_file.write_text("[]")

        mock_load.return_value = [
            WalletConfig(
                blockchain="ethereum",
                friendly_name="wallet",
                address="0xabc",
                provider="zerion",
            ),
        ]
        mock_sync.return_value = [tmp_path / "output" / "ethereum_0xabc_transactions.csv"]

        main([str(config_file), "-o", str(tmp_path / "output")])

        call_kwargs = mock_sync.call_args[1]
        assert call_kwargs["zerion_api_key"] == "zk_dev_test"
