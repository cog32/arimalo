"""Tests for sync_crypto.clients."""
import json
from pathlib import Path
from unittest.mock import patch, MagicMock

import pytest

from sync_crypto.models import TransactionRecord

FIXTURES = Path(__file__).parent / "fixtures"


def load_fixture(name: str) -> dict:
    return json.loads((FIXTURES / name).read_text())


class TestRateLimiter:
    def test_rate_limiter_enforces_delay(self):
        from sync_crypto.clients import RateLimiter
        import time

        limiter = RateLimiter(calls_per_second=10)
        start = time.monotonic()
        limiter.wait()
        limiter.wait()
        elapsed = time.monotonic() - start
        assert elapsed >= 0.1  # At least 1/10 second between calls

    def test_rate_limiter_first_call_immediate(self):
        from sync_crypto.clients import RateLimiter
        import time

        limiter = RateLimiter(calls_per_second=3)
        start = time.monotonic()
        limiter.wait()
        elapsed = time.monotonic() - start
        assert elapsed < 0.1  # First call should be immediate


class TestEtherscanClient:
    def _make_client(self):
        from sync_crypto.clients import EtherscanClient
        return EtherscanClient(api_key="test_key")

    def test_chain_id_for_network_supports_common_aliases(self):
        from sync_crypto.clients import EtherscanClient

        assert EtherscanClient.chain_id_for_network("ethereum") == 1
        assert EtherscanClient.chain_id_for_network("Base") == 8453
        assert EtherscanClient.chain_id_for_network("arbitrum_one") == 42161
        assert EtherscanClient.chain_id_for_network("unknown") is None

    def test_normalize_normal_tx(self):
        client = self._make_client()
        fixture = load_fixture("etherscan_normal_tx.json")
        raw = fixture["result"][0]
        tx = client._normalize_normal_tx(raw)
        assert isinstance(tx, TransactionRecord)
        assert tx.tx_hash == "0xabc123normal"
        assert tx.blockchain == "ethereum"
        assert tx.timestamp == 1700000000
        assert tx.from_address == "0xfromaddr"
        assert tx.to_address == "0xtoaddr"
        assert tx.value == "1000000000000000000"
        assert tx.fee == str(int("21000") * int("20000000000"))
        assert tx.status == "success"
        assert tx.tx_type == "normal"
        assert tx.block_number == 18500000
        assert tx.gas_used == "21000"
        assert tx.gas_price == "20000000000"
        assert tx.method == "transfer"
        assert tx.currency == "ETH"
        assert tx.method_id == "0xa9059cbb"
        assert tx.function_name == "transfer(address,uint256)"
        assert tx.input_data == "0xabcdef"
        assert tx.tx_receipt_status == "1"
        assert tx.transaction_index == 1
        assert tx.cumulative_gas_used == "50000"
        assert tx.confirmations == 1234

    def test_normalize_internal_tx(self):
        client = self._make_client()
        fixture = load_fixture("etherscan_internal_tx.json")
        raw = fixture["result"][0]
        tx = client._normalize_internal_tx(raw)
        assert tx.tx_hash == "0xabc123internal"
        assert tx.tx_type == "internal"
        assert tx.value == "500000000000000000"
        assert tx.status == "success"
        assert tx.currency == "ETH"
        assert tx.transaction_index == 2
        assert tx.cumulative_gas_used == "90000"
        assert tx.confirmations == 900
        assert tx.input_data == "0x1234"

    def test_normalize_token_tx(self):
        client = self._make_client()
        fixture = load_fixture("etherscan_token_tx.json")
        raw = fixture["result"][0]
        tx = client._normalize_token_tx(raw)
        assert tx.tx_hash == "0xabc123token"
        assert tx.tx_type == "token_transfer"
        assert tx.token_name == "USD Coin"
        assert tx.token_symbol == "USDC"
        assert tx.token_decimals == 6
        assert tx.token_contract == "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"
        assert tx.value == "500000000"
        assert tx.currency == "USDC"
        assert tx.method_id == "0xa9059cbb"
        assert tx.function_name == "transfer(address to, uint256 value)"
        assert tx.input_data is None
        assert tx.tx_receipt_status == "1"
        assert tx.transaction_index == 3
        assert tx.cumulative_gas_used == "142000"
        assert tx.confirmations == 2345

    def test_normalize_token_tx_currency_falls_back_to_contract(self):
        client = self._make_client()
        raw = {
            "blockNumber": "18500003",
            "timeStamp": "1700000300",
            "hash": "0xabc123nosymbol",
            "from": "0xfromaddr",
            "to": "0xtoaddr",
            "value": "1",
            "gasPrice": "10",
            "gasUsed": "20",
            "tokenName": "",
            "tokenSymbol": "   ",
            "tokenDecimal": "18",
            "contractAddress": "0xcontract",
        }
        tx = client._normalize_token_tx(raw)
        assert tx.currency == "0xcontract"
        assert tx.token_symbol is None
        assert tx.token_name is None

    def test_normalize_failed_tx(self):
        client = self._make_client()
        raw = {
            "blockNumber": "18500000",
            "timeStamp": "1700000000",
            "hash": "0xfailed",
            "from": "0xfrom",
            "to": "0xto",
            "value": "0",
            "gas": "21000",
            "gasPrice": "20000000000",
            "gasUsed": "21000",
            "isError": "1",
            "txreceipt_status": "0",
            "functionName": "",
            "methodId": "0x",
        }
        tx = client._normalize_normal_tx(raw)
        assert tx.status == "failed"

    @patch("sync_crypto.clients.requests.get")
    def test_fetch_normal_transactions(self, mock_get):
        mock_response = MagicMock()
        mock_response.json.return_value = load_fixture("etherscan_normal_tx.json")
        mock_response.raise_for_status = MagicMock()
        mock_get.return_value = mock_response

        client = self._make_client()
        txs = client._fetch_normal_transactions("0xfromaddr")
        assert len(txs) == 1
        assert txs[0].tx_hash == "0xabc123normal"

        # Verify API was called with correct params
        call_args = mock_get.call_args
        params = call_args[1]["params"]
        assert params["module"] == "account"
        assert params["action"] == "txlist"
        assert params["address"] == "0xfromaddr"

    @patch("sync_crypto.clients.requests.get")
    def test_fetch_all_transactions_combines_types(self, mock_get):
        """fetch_all_transactions should combine normal, internal, and token txs."""
        normal = load_fixture("etherscan_normal_tx.json")
        internal = load_fixture("etherscan_internal_tx.json")
        token = load_fixture("etherscan_token_tx.json")

        mock_response = MagicMock()
        mock_response.raise_for_status = MagicMock()
        mock_response.json.side_effect = [normal, internal, token]
        mock_get.return_value = mock_response

        client = self._make_client()
        txs = client.fetch_all_transactions("0xfromaddr")
        assert len(txs) == 3
        types = {tx.tx_type for tx in txs}
        assert types == {"normal", "internal", "token_transfer"}

    @patch("sync_crypto.clients.requests.get")
    def test_fetch_empty_response(self, mock_get):
        mock_response = MagicMock()
        mock_response.json.return_value = load_fixture("etherscan_empty.json")
        mock_response.raise_for_status = MagicMock()
        mock_get.return_value = mock_response

        client = self._make_client()
        txs = client._fetch_normal_transactions("0xempty")
        assert txs == []

    @patch("sync_crypto.clients.requests.get")
    def test_fetch_transaction_metadata_uses_proxy_endpoints(self, mock_get):
        tx_response = MagicMock()
        tx_response.raise_for_status = MagicMock()
        tx_response.json.return_value = {
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "hash": "0xmeta",
                "blockNumber": "0x64",
                "gasPrice": "0x4a817c800",
                "input": "0xa9059cbb0000000000000000",
                "transactionIndex": "0x7",
            },
        }

        receipt_response = MagicMock()
        receipt_response.raise_for_status = MagicMock()
        receipt_response.json.return_value = {
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "gasUsed": "0x5208",
                "effectiveGasPrice": "0x3b9aca00",
                "status": "0x1",
                "cumulativeGasUsed": "0x927c0",
                "transactionIndex": "0x7",
            },
        }

        block_response = MagicMock()
        block_response.raise_for_status = MagicMock()
        block_response.json.return_value = {
            "jsonrpc": "2.0",
            "id": 1,
            "result": "0x6e",
        }

        mock_get.side_effect = [tx_response, receipt_response, block_response]

        client = self._make_client()
        metadata = client.fetch_transaction_metadata("0xmeta")

        assert metadata == {
            "block_number": 100,
            "gas_used": "21000",
            "gas_price": "1000000000",
            "input_data": "0xa9059cbb0000000000000000",
            "tx_receipt_status": "1",
            "transaction_index": 7,
            "cumulative_gas_used": "600000",
            "confirmations": 10,
            "method_id": "0xa9059cbb",
        }

        first_params = mock_get.call_args_list[0].kwargs["params"]
        second_params = mock_get.call_args_list[1].kwargs["params"]
        third_params = mock_get.call_args_list[2].kwargs["params"]
        assert first_params["module"] == "proxy"
        assert first_params["action"] == "eth_getTransactionByHash"
        assert first_params["txhash"] == "0xmeta"
        assert second_params["action"] == "eth_getTransactionReceipt"
        assert third_params["action"] == "eth_blockNumber"

    @patch("sync_crypto.clients.requests.get")
    def test_pagination_fetches_multiple_pages(self, mock_get):
        """When first page returns max results, client should fetch next page."""
        # First page: full page of 10000
        page1_result = load_fixture("etherscan_normal_tx.json")["result"] * 10000
        page1 = {"status": "1", "message": "OK", "result": page1_result}
        # Second page: partial (end of data)
        page2 = load_fixture("etherscan_normal_tx.json")

        mock_response = MagicMock()
        mock_response.raise_for_status = MagicMock()
        mock_response.json.side_effect = [page1, page2]
        mock_get.return_value = mock_response

        client = self._make_client()
        txs = client._fetch_normal_transactions("0xaddr")
        assert len(txs) == 10001
        assert mock_get.call_count == 2


class TestSolanaRpcClient:
    def _make_client(self):
        from sync_crypto.clients import SolanaRpcClient
        return SolanaRpcClient()

    @patch.dict("os.environ", {}, clear=True)
    def test_resolve_rpc_url_prefers_explicit_url(self):
        from sync_crypto.clients import SolanaRpcClient

        assert SolanaRpcClient.resolve_rpc_url("https://example-rpc.test") == "https://example-rpc.test"

    @patch.dict("os.environ", {"SOLANA_RPC_URL": "https://env-rpc.test"}, clear=True)
    def test_resolve_rpc_url_uses_solana_rpc_url_env(self):
        from sync_crypto.clients import SolanaRpcClient

        assert SolanaRpcClient.resolve_rpc_url() == "https://env-rpc.test"

    @patch.dict(
        "os.environ",
        {
            "SOLANA_RPC_URL": "https://env-rpc.test",
            "HELIUS_API_KEY": "helius_test_key",
        },
        clear=True,
    )
    def test_resolve_rpc_url_prefers_helius_api_key_over_env_url(self):
        from sync_crypto.clients import SolanaRpcClient

        assert (
            SolanaRpcClient.resolve_rpc_url()
            == "https://mainnet.helius-rpc.com/?api-key=helius_test_key"
        )

    @patch.dict("os.environ", {"HELIUS_API_KEY": "helius_test_key"}, clear=True)
    def test_resolve_rpc_url_builds_helius_url_from_api_key(self):
        from sync_crypto.clients import SolanaRpcClient

        assert (
            SolanaRpcClient.resolve_rpc_url()
            == "https://mainnet.helius-rpc.com/?api-key=helius_test_key"
        )

    def test_parse_native_sol_transfer(self):
        """Parse a native SOL transfer from pre/post balance comparison."""
        client = self._make_client()
        fixture = load_fixture("solana_rpc_native_tx.json")
        tx_data = fixture["result"]
        records = client._parse_transaction(tx_data, "7xKsender", "5abc123solana")
        assert len(records) == 1
        tx = records[0]
        assert tx.tx_hash == "5abc123solana"
        assert tx.blockchain == "solana"
        assert tx.timestamp == 1700000000
        assert tx.from_address == "7xKsender"
        assert tx.to_address == "8yLreceiver"
        assert tx.value == "1000000000"
        assert tx.fee == "5000"
        assert tx.status == "success"
        assert tx.tx_type == "native"
        assert tx.block_number == 230000000
        assert tx.currency == "SOL"

    def test_parse_native_sol_receive(self):
        """When queried from receiver's perspective, from/to are correct."""
        client = self._make_client()
        fixture = load_fixture("solana_rpc_native_tx.json")
        tx_data = fixture["result"]
        records = client._parse_transaction(tx_data, "8yLreceiver", "5abc123solana")
        assert len(records) == 1
        tx = records[0]
        assert tx.from_address == "7xKsender"
        assert tx.to_address == "8yLreceiver"
        assert tx.value == "1000000000"

    def test_parse_token_transfer(self):
        """Parse an SPL token transfer from pre/post token balances."""
        client = self._make_client()
        fixture = load_fixture("solana_rpc_token_tx.json")
        tx_data = fixture["result"]
        records = client._parse_transaction(tx_data, "7xKsender", "5def456soltoken")
        assert len(records) == 1
        tx = records[0]
        assert tx.tx_hash == "5def456soltoken"
        assert tx.tx_type == "token_transfer"
        assert tx.from_address == "7xKsender"
        assert tx.to_address == "8yLreceiver"
        assert tx.value == "1000000"
        assert tx.token_contract == "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
        assert tx.token_decimals == 6
        assert tx.token_name is None  # RPC doesn't provide token metadata
        assert tx.token_symbol is None
        assert tx.fee == "5000"
        assert tx.currency == "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"

    def test_parse_failed_transaction(self):
        """Failed transactions should have status='failed'."""
        client = self._make_client()
        fixture = load_fixture("solana_rpc_failed_tx.json")
        tx_data = fixture["result"]
        records = client._parse_transaction(tx_data, "7xKsender", "5failed789")
        assert len(records) == 1
        tx = records[0]
        assert tx.status == "failed"
        assert tx.tx_hash == "5failed789"
        assert tx.from_address == "7xKsender"
        assert tx.fee == "5000"
        assert tx.currency == "SOL"

    def test_parse_null_transaction(self):
        """Null transaction data (pruned) should return empty list."""
        client = self._make_client()
        records = client._parse_transaction(None, "7xKsender", "5pruned")
        assert records == []

    @patch("sync_crypto.clients.requests.post")
    def test_fetch_signatures_pagination(self, mock_post):
        """Signatures should paginate using 'before' cursor."""
        # First page: full (1000 sigs)
        page1_sig = load_fixture("solana_rpc_signatures.json")
        full_page = page1_sig.copy()
        full_page["result"] = page1_sig["result"] * 1000
        # Give last sig a unique signature for cursor
        full_page["result"][-1] = {**full_page["result"][-1], "signature": "5last_cursor"}
        # Second page: partial (end of data)
        page2 = load_fixture("solana_rpc_signatures.json")

        mock_response = MagicMock()
        mock_response.raise_for_status = MagicMock()
        mock_response.json.side_effect = [full_page, page2]
        mock_post.return_value = mock_response

        client = self._make_client()
        sigs = client._fetch_signatures("7xKsender")
        assert len(sigs) == 1001
        assert mock_post.call_count == 2

        # Verify second call used 'before' cursor
        second_call_payload = mock_post.call_args_list[1][1]["json"]
        assert second_call_payload["params"][1]["before"] == "5last_cursor"

    @patch("sync_crypto.clients.requests.post")
    def test_fetch_transactions_chunked_yields_batches(self, mock_post):
        """Chunked fetch yields batches of TransactionRecords."""
        sigs_response = load_fixture("solana_rpc_signatures.json")
        native_tx_response = load_fixture("solana_rpc_native_tx.json")

        mock_response = MagicMock()
        mock_response.raise_for_status = MagicMock()
        mock_response.json.side_effect = [sigs_response, native_tx_response]
        mock_post.return_value = mock_response

        client = self._make_client()
        chunks = list(client.fetch_transactions_chunked("7xKsender"))
        assert len(chunks) == 1
        assert len(chunks[0]) == 1
        assert chunks[0][0].tx_hash == "5abc123solana"

    @patch("sync_crypto.clients.requests.post")
    def test_fetch_transactions_chunked_skips_failures(self, mock_post):
        """Individual transaction fetch failures should be skipped."""
        # 2 signatures
        sigs = {
            "jsonrpc": "2.0",
            "result": [
                {"signature": "5good", "slot": 1, "blockTime": 1, "err": None,
                 "memo": None, "confirmationStatus": "finalized"},
                {"signature": "5bad", "slot": 2, "blockTime": 2, "err": None,
                 "memo": None, "confirmationStatus": "finalized"},
            ],
            "id": 1,
        }
        native_tx = load_fixture("solana_rpc_native_tx.json")

        # Signatures succeed, first tx succeeds, second tx raises
        success_resp = MagicMock()
        success_resp.status_code = 200
        success_resp.raise_for_status = MagicMock()

        fail_resp = MagicMock()
        fail_resp.status_code = 500
        fail_resp.raise_for_status.side_effect = Exception("Server error")

        success_resp.json.side_effect = [sigs, native_tx]
        mock_post.side_effect = [success_resp, success_resp, fail_resp]

        client = self._make_client()
        client.CHUNK_SIZE = 25  # both sigs in one chunk
        all_txs = []
        for chunk in client.fetch_transactions_chunked("7xKsender"):
            all_txs.extend(chunk)
        # Only the successful tx should appear
        assert len(all_txs) == 1
        assert all_txs[0].tx_hash == "5good"

    @patch("sync_crypto.clients.requests.post")
    def test_fetch_transactions_chunked_empty(self, mock_post):
        """Address with no transactions yields no chunks."""
        empty_response = load_fixture("solana_rpc_empty.json")

        mock_response = MagicMock()
        mock_response.raise_for_status = MagicMock()
        mock_response.json.return_value = empty_response
        mock_post.return_value = mock_response

        client = self._make_client()
        chunks = list(client.fetch_transactions_chunked("EmptyAddress"))
        assert chunks == []

    @patch("sync_crypto.clients.requests.post")
    def test_rpc_error_raises(self, mock_post):
        """RPC error response should raise RuntimeError."""
        error_response = {
            "jsonrpc": "2.0",
            "error": {"code": -32600, "message": "Invalid request"},
            "id": 1,
        }
        mock_response = MagicMock()
        mock_response.raise_for_status = MagicMock()
        mock_response.json.return_value = error_response
        mock_post.return_value = mock_response

        client = self._make_client()
        with pytest.raises(RuntimeError, match="RPC error"):
            client._fetch_signatures("7xKsender")

    @patch("sync_crypto.clients.time.sleep")
    @patch("sync_crypto.clients.requests.post")
    def test_retries_on_429(self, mock_post, mock_sleep):
        """429 responses should be retried with exponential backoff."""
        rate_limited = MagicMock()
        rate_limited.status_code = 429

        success = MagicMock()
        success.status_code = 200
        success.raise_for_status = MagicMock()
        success.json.return_value = load_fixture("solana_rpc_signatures.json")

        mock_post.side_effect = [rate_limited, rate_limited, success]

        client = self._make_client()
        sigs = client._fetch_signatures("7xKsender")
        assert len(sigs) == 1
        assert mock_post.call_count == 3
        # Backoff: 2^0+5=6s, 2^1+5=7s
        assert mock_sleep.call_args_list[0][0][0] == 6
        assert mock_sleep.call_args_list[1][0][0] == 7

    @patch("sync_crypto.clients.requests.post")
    def test_fetch_signatures_with_until(self, mock_post):
        """When until is provided, it should appear in the RPC opts."""
        sigs_response = load_fixture("solana_rpc_signatures.json")

        mock_response = MagicMock()
        mock_response.raise_for_status = MagicMock()
        mock_response.json.return_value = sigs_response
        mock_post.return_value = mock_response

        client = self._make_client()
        sigs = client._fetch_signatures("7xKsender", until="5lastknown")
        assert len(sigs) == 1

        # Verify 'until' was passed in the RPC call
        call_payload = mock_post.call_args[1]["json"]
        assert call_payload["params"][1]["until"] == "5lastknown"

    @patch("sync_crypto.clients.requests.post")
    def test_fetch_signatures_without_until(self, mock_post):
        """When until is None, 'until' should not appear in opts."""
        sigs_response = load_fixture("solana_rpc_signatures.json")

        mock_response = MagicMock()
        mock_response.raise_for_status = MagicMock()
        mock_response.json.return_value = sigs_response
        mock_post.return_value = mock_response

        client = self._make_client()
        client._fetch_signatures("7xKsender")

        call_payload = mock_post.call_args[1]["json"]
        assert "until" not in call_payload["params"][1]

    @patch("sync_crypto.clients.requests.post")
    def test_fetch_transactions_chunked_with_until(self, mock_post):
        """fetch_transactions_chunked should pass until to _fetch_signatures."""
        sigs_response = load_fixture("solana_rpc_signatures.json")
        native_tx_response = load_fixture("solana_rpc_native_tx.json")

        mock_response = MagicMock()
        mock_response.raise_for_status = MagicMock()
        mock_response.json.side_effect = [sigs_response, native_tx_response]
        mock_post.return_value = mock_response

        client = self._make_client()
        chunks = list(client.fetch_transactions_chunked("7xKsender", until="5lastknown"))
        assert len(chunks) == 1

        # First RPC call should be getSignaturesForAddress with until
        first_call_payload = mock_post.call_args_list[0][1]["json"]
        assert first_call_payload["params"][1]["until"] == "5lastknown"


class TestEtherscanStartBlock:
    def _make_client(self):
        from sync_crypto.clients import EtherscanClient
        return EtherscanClient(api_key="test_key")

    @patch("sync_crypto.clients.requests.get")
    def test_fetch_paginated_with_start_block(self, mock_get):
        """_fetch_paginated should use provided start_block."""
        mock_response = MagicMock()
        mock_response.json.return_value = load_fixture("etherscan_normal_tx.json")
        mock_response.raise_for_status = MagicMock()
        mock_get.return_value = mock_response

        client = self._make_client()
        client._fetch_paginated("txlist", "0xaddr", client._normalize_normal_tx, start_block=18500000)

        params = mock_get.call_args[1]["params"]
        assert params["startblock"] == 18500000

    @patch("sync_crypto.clients.requests.get")
    def test_fetch_paginated_default_start_block_zero(self, mock_get):
        """Default start_block should be 0."""
        mock_response = MagicMock()
        mock_response.json.return_value = load_fixture("etherscan_normal_tx.json")
        mock_response.raise_for_status = MagicMock()
        mock_get.return_value = mock_response

        client = self._make_client()
        client._fetch_paginated("txlist", "0xaddr", client._normalize_normal_tx)

        params = mock_get.call_args[1]["params"]
        assert params["startblock"] == 0

    @patch("sync_crypto.clients.requests.get")
    def test_fetch_all_transactions_with_start_block(self, mock_get):
        """fetch_all_transactions should pass start_block through."""
        normal = load_fixture("etherscan_normal_tx.json")
        internal = load_fixture("etherscan_internal_tx.json")
        token = load_fixture("etherscan_token_tx.json")

        mock_response = MagicMock()
        mock_response.raise_for_status = MagicMock()
        mock_response.json.side_effect = [normal, internal, token]
        mock_get.return_value = mock_response

        client = self._make_client()
        client.fetch_all_transactions("0xaddr", start_block=18500000)

        # All three calls should use start_block
        for call in mock_get.call_args_list:
            params = call[1]["params"]
            assert params["startblock"] == 18500000


OUR_OSMO_ADDR = "osmo12grf7p9gndy2uy4yj4vf6danepphx0yumf3ztr"


class TestMintscanClient:
    def _make_client(self):
        from sync_crypto.clients import MintscanClient
        return MintscanClient(api_key="test_jwt_token")

    def test_network_for_address_osmosis(self):
        client = self._make_client()
        assert client._network_for_address("osmo1abc") == "osmosis"

    def test_network_for_address_cosmos(self):
        client = self._make_client()
        assert client._network_for_address("cosmos1abc") == "cosmos"

    def test_network_for_address_unknown_raises(self):
        client = self._make_client()
        with pytest.raises(ValueError, match="Unknown Cosmos address prefix"):
            client._network_for_address("unknown1abc")

    def test_parse_amount_string(self):
        from sync_crypto.clients import MintscanClient
        assert MintscanClient._parse_amount_string("1709046927uosmo") == ("1709046927", "uosmo")
        assert MintscanClient._parse_amount_string("33ibc/27394FB092D") == ("33", "ibc/27394FB092D")
        assert MintscanClient._parse_amount_string("") == ("0", "")

    def test_normalize_transaction_with_transfers(self):
        client = self._make_client()
        fixture = load_fixture("mintscan_transactions_page1.json")
        raw_tx = fixture["transactions"][0]
        records = client._normalize_transaction(raw_tx, OUR_OSMO_ADDR)
        # TX 0 has one transfer involving our address (the other goes to osmo1someotheraddr)
        assert len(records) == 1
        rec = records[0]
        assert isinstance(rec, TransactionRecord)
        assert rec.tx_hash == "12B98A0128E077EF973665FB1C1674D08A21DAE2AEC6DC670A8C3A80B7207E7E"
        assert rec.blockchain == "cosmos"
        assert rec.timestamp == 1774868332  # 2026-03-30T10:58:52Z
        assert rec.from_address == "osmo1c3ljch9dfw5kf52nfwpxd2zmj2ese7agnx0p9tenkrryasrle5sqf3ftpg"
        assert rec.to_address == OUR_OSMO_ADDR
        assert rec.value == "1709046927"
        assert rec.currency == "uosmo"
        assert rec.fee == "2407102"
        assert rec.status == "success"
        assert rec.block_number == 58189549
        assert rec.gas_used == "17197905"

    def test_normalize_transaction_no_transfers_for_our_address(self):
        client = self._make_client()
        fixture = load_fixture("mintscan_transactions_page1.json")
        raw_tx = fixture["transactions"][1]  # IBC message, no transfers to our address
        records = client._normalize_transaction(raw_tx, OUR_OSMO_ADDR)
        assert len(records) == 0

    def test_normalize_transaction_failed_tx(self):
        client = self._make_client()
        fixture = load_fixture("mintscan_transactions_page2.json")
        raw_tx = fixture["transactions"][0]  # code=5, failed
        records = client._normalize_transaction(raw_tx, OUR_OSMO_ADDR)
        # Failed tx with no transfer events still records the tx
        assert len(records) == 1
        assert records[0].status == "failed"
        assert records[0].fee == "5000"

    @patch("sync_crypto.clients.requests.get")
    def test_request_sends_bearer_auth(self, mock_get):
        client = self._make_client()
        mock_response = MagicMock()
        mock_response.status_code = 200
        mock_response.raise_for_status = MagicMock()
        mock_response.json.return_value = {"transactions": [], "pagination": {}}
        mock_get.return_value = mock_response

        client._request("/v1/osmosis/accounts/osmo1abc/transactions")

        call_kwargs = mock_get.call_args[1]
        assert "Authorization" in call_kwargs["headers"]
        assert call_kwargs["headers"]["Authorization"] == "Bearer test_jwt_token"

    @patch("sync_crypto.clients.requests.get")
    def test_fetch_transactions_chunked_pagination(self, mock_get):
        client = self._make_client()
        page1 = load_fixture("mintscan_transactions_page1.json")
        page2 = load_fixture("mintscan_transactions_page2.json")

        mock_response = MagicMock()
        mock_response.status_code = 200
        mock_response.raise_for_status = MagicMock()
        mock_response.json.side_effect = [page1, page2]
        mock_get.return_value = mock_response

        chunks = list(client.fetch_transactions_chunked(OUR_OSMO_ADDR))
        all_records = [rec for chunk in chunks for rec in chunk]
        # Page 1: 1 transfer involving our addr; Page 2: 1 failed tx
        assert len(all_records) == 2

    @patch("sync_crypto.clients.requests.get")
    def test_fetch_transactions_chunked_stops_at_until(self, mock_get):
        client = self._make_client()
        page1 = load_fixture("mintscan_transactions_page1.json")

        mock_response = MagicMock()
        mock_response.status_code = 200
        mock_response.raise_for_status = MagicMock()
        mock_response.json.return_value = page1
        mock_get.return_value = mock_response

        # Set until to the second tx hash — should stop before it
        until_hash = "FB4DE54A75DD505F667459BE9AC8331BA6BC70BAF73636C246BD72D7A3A3B58E"
        chunks = list(client.fetch_transactions_chunked(OUR_OSMO_ADDR, until=until_hash))
        all_records = [rec for chunk in chunks for rec in chunk]
        # Only the first tx should be included
        assert len(all_records) == 1
        assert all_records[0].tx_hash.startswith("12B98A01")
