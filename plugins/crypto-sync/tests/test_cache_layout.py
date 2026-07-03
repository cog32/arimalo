"""Round-trip tests for sync_crypto.cache."""
import json
from pathlib import Path

from sync_crypto.cache import (
    CACHE_SCHEMA_VERSION,
    ETHERSCAN_PROVIDER,
    RawResponseCache,
    SOLANA_RPC_PROVIDER,
    ZERION_PROVIDER,
    _normalize_address,
)
from sync_crypto.clients import EtherscanClient, SolanaRpcClient
from sync_crypto.models import WalletConfig
from sync_crypto.zerion import ZerionClient


def test_zerion_page_round_trip_preserves_payload(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")
    address = "0xWalletAddress"
    payload = {
        "data": [{"id": "tx-1", "type": "transactions"}],
        "links": {"next": None},
    }

    cache.write_zerion_page(address, 1, payload)
    read_back = cache.read_zerion_page(address, 1)

    assert read_back == payload


def test_read_zerion_page_returns_none_when_absent(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")
    assert cache.read_zerion_page("0xabc", 1) is None


def test_iter_zerion_pages_yields_in_index_order(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")
    address = "0xabc"
    cache.write_zerion_page(address, 3, {"page": 3})
    cache.write_zerion_page(address, 1, {"page": 1})
    cache.write_zerion_page(address, 2, {"page": 2})
    cache.write_zerion_index(address, [3, 1, 2])

    pages = list(cache.iter_zerion_pages(address))

    assert pages == [{"page": 1}, {"page": 2}, {"page": 3}]


def test_iter_zerion_pages_returns_empty_when_no_index(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")
    assert list(cache.iter_zerion_pages("0xabc")) == []


def test_index_records_schema_version_provider_and_address(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")
    cache.write_zerion_index("0xWalletAddress", [1, 2])

    index = cache.read_zerion_index("0xWalletAddress")
    assert index is not None
    assert index["cache_schema_version"] == CACHE_SCHEMA_VERSION
    assert index["provider"] == ZERION_PROVIDER
    assert index["address"] == "0xwalletaddress"
    assert index["pages"] == [1, 2]
    assert isinstance(index.get("last_fetch_ts"), int)


def test_address_normalized_to_lowercase_in_paths(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")
    cache.write_zerion_page("0xWalletAddress", 1, {"x": 1})

    expected_path = tmp_path / "cache" / "zerion" / "0xwalletaddress" / "page-0001.json"
    assert expected_path.exists()


def test_index_file_is_human_readable_json(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")
    cache.write_zerion_index("0xabc", [1, 2, 3])

    raw = (tmp_path / "cache" / "zerion" / "0xabc" / "index.json").read_text()
    parsed = json.loads(raw)
    assert "\n" in raw
    assert parsed["pages"] == [1, 2, 3]


def _eth_transfer_page(
    *,
    tx_id: str,
    tx_hash: str,
    mined_at: str,
    next_url: str | None = None,
) -> dict:
    return {
        "data": [
            {
                "id": tx_id,
                "type": "transactions",
                "attributes": {
                    "hash": tx_hash,
                    "status": "confirmed",
                    "mined_at": mined_at,
                    "mined_at_block": 18500000,
                    "operation_type": "transfer",
                    "fee": {"value": "21000000000000"},
                    "transfers": [
                        {
                            "from": "0xfrom",
                            "to": "0xto",
                            "quantity": {"numeric": "1.5", "decimals": 18},
                            "fungible_info": {
                                "symbol": "ETH",
                                "name": "Ether",
                                "implementations": [],
                            },
                        }
                    ],
                },
                "relationships": {"chain": {"data": {"id": "ethereum"}}},
            }
        ],
        "links": {"next": next_url},
    }


def test_fetch_new_transaction_batches_consumes_cached_pages(tmp_path: Path):
    """Drive ``fetch_new_transaction_batches`` from cached pages — no API calls."""
    cache = RawResponseCache(tmp_path / "cache")
    address = "0xwallet"

    cache.write_zerion_page(address, 1, _eth_transfer_page(
        tx_id="tx-1", tx_hash="0xhash1", mined_at="2025-01-03T00:00:00Z",
    ))
    cache.write_zerion_index(address, [1])

    client = ZerionClient(api_key="zk_dev_test")
    wallet = WalletConfig(
        blockchain="ethereum",
        friendly_name="main",
        address=address,
        provider="zerion",
        network="ethereum",
    )

    batches = list(client.fetch_new_transaction_batches(
        wallet,
        page_source=cache.iter_zerion_pages(address),
    ))

    assert len(batches) == 1
    assert batches[0][0].record_id == "zerion:tx-1:0"
    assert batches[0][0].tx_hash == "0xhash1"


def test_fetch_new_transaction_batches_walks_multiple_cached_pages(tmp_path: Path):
    """Multi-page cached read: state-boundary on the second page stops iteration."""
    cache = RawResponseCache(tmp_path / "cache")
    address = "0xwallet"

    cache.write_zerion_page(address, 1, _eth_transfer_page(
        tx_id="tx-new", tx_hash="0xnew", mined_at="2025-01-03T00:00:00Z",
    ))
    cache.write_zerion_page(address, 2, _eth_transfer_page(
        tx_id="tx-old", tx_hash="0xold", mined_at="2025-01-01T00:00:00Z",
    ))
    cache.write_zerion_index(address, [1, 2])

    client = ZerionClient(api_key="zk_dev_test")
    wallet = WalletConfig(
        blockchain="ethereum",
        friendly_name="main",
        address=address,
        provider="zerion",
        network="ethereum",
    )

    batches = list(client.fetch_new_transaction_batches(
        wallet,
        state={
            "latest_timestamp": 1735689600,
            "latest_record_ids": ["zerion:tx-old:0"],
        },
        page_source=cache.iter_zerion_pages(address),
    ))

    assert len(batches) == 1
    assert batches[0][0].record_id == "zerion:tx-new:0"


# ===== Address normalization =====


def test_normalize_address_lowercases_evm_hex():
    assert _normalize_address("0xWalletAddress") == "0xwalletaddress"
    assert _normalize_address("0XABC") == "0xabc"


def test_normalize_address_preserves_solana_base58():
    sol_addr = "5xKsenderaBcDeFgHiJkLmN7xKpqrstuvwxyz12345"
    assert _normalize_address(sol_addr) == sol_addr


# ===== Etherscan paginated =====


def test_etherscan_page_round_trip_preserves_payload(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")
    payload = {"status": "1", "message": "OK", "result": [{"hash": "0xabc"}]}

    cache.write_etherscan_page(1, "0xWallet", "txlist", 1, payload)
    read_back = cache.read_etherscan_page(1, "0xWallet", "txlist", 1)

    assert read_back == payload


def test_etherscan_iter_pages_yields_in_index_order(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")
    cache.write_etherscan_page(1, "0xabc", "tokentx", 2, {"page": 2})
    cache.write_etherscan_page(1, "0xabc", "tokentx", 1, {"page": 1})
    cache.write_etherscan_index(1, "0xabc", "tokentx", [2, 1])

    pages = list(cache.iter_etherscan_pages(1, "0xabc", "tokentx"))

    assert pages == [{"page": 1}, {"page": 2}]


def test_etherscan_index_records_chain_and_action(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")
    cache.write_etherscan_index(8453, "0xWallet", "txlistinternal", [1])

    index = cache.read_etherscan_index(8453, "0xWallet", "txlistinternal")
    assert index is not None
    assert index["chain_id"] == 8453
    assert index["action"] == "txlistinternal"
    assert index["address"] == "0xwallet"
    assert index["provider"] == ETHERSCAN_PROVIDER
    assert index["cache_schema_version"] == CACHE_SCHEMA_VERSION


def test_etherscan_path_layout(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")
    cache.write_etherscan_page(1, "0xWalletAddress", "txlist", 1, {"x": 1})

    expected = tmp_path / "cache" / "etherscan" / "1" / "0xwalletaddress" / "txlist" / "page-0001.json"
    assert expected.exists()


def test_fetch_paginated_consumes_cached_pages(tmp_path: Path):
    """Drive _fetch_normal_transactions from cached pages — no API calls."""
    cache = RawResponseCache(tmp_path / "cache")
    address = "0xwallet"
    page1 = {
        "status": "1",
        "message": "OK",
        "result": [{
            "blockNumber": "18500000",
            "timeStamp": "1700000000",
            "hash": "0xabc",
            "from": "0xfrom",
            "to": "0xto",
            "value": "1000000000000000000",
            "gas": "21000",
            "gasPrice": "20000000000",
            "gasUsed": "21000",
            "isError": "0",
            "txreceipt_status": "1",
            "input": "0x",
            "functionName": "",
            "methodId": "0x",
            "transactionIndex": "0",
            "cumulativeGasUsed": "21000",
            "confirmations": "1",
            "nonce": "0",
        }],
    }

    cache.write_etherscan_page(1, address, "txlist", 1, page1)
    cache.write_etherscan_index(1, address, "txlist", [1])

    client = EtherscanClient(api_key="test_key")
    records = client._fetch_normal_transactions(
        address,
        page_source=cache.iter_etherscan_pages(1, address, "txlist"),
    )

    assert len(records) == 1
    assert records[0].tx_hash == "0xabc"
    assert records[0].value == "1000000000000000000"


# ===== Etherscan proxy =====


def test_etherscan_proxy_round_trip_preserves_payload(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")
    receipt = {"status": "0x1", "gasUsed": "0x5208", "blockNumber": "0x11a8240"}

    cache.write_etherscan_proxy(1, "eth_getTransactionReceipt", "0xabc", receipt)
    read_back = cache.read_etherscan_proxy(1, "eth_getTransactionReceipt", "0xabc")

    assert read_back == receipt


def test_etherscan_proxy_returns_none_when_absent(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")
    assert cache.read_etherscan_proxy(1, "eth_getTransactionReceipt", "0xmissing") is None


def test_etherscan_proxy_path_layout(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")
    cache.write_etherscan_proxy(1, "eth_blockNumber", "latest", "0x11a8240")

    expected = tmp_path / "cache" / "etherscan" / "1" / "proxy" / "eth_blockNumber" / "latest.json"
    assert expected.exists()


def test_etherscan_proxy_distinguishes_null_from_missing(tmp_path: Path):
    """Cached null (e.g. getTransactionByHash for a non-existent hash) must
    be distinguishable from a cache miss."""
    cache = RawResponseCache(tmp_path / "cache")
    cache.write_etherscan_proxy(1, "eth_getTransactionByHash", "0xnonexistent", None)

    assert cache.read_etherscan_proxy(1, "eth_getTransactionByHash", "0xnonexistent") is None
    assert cache.has_etherscan_proxy(1, "eth_getTransactionByHash", "0xnonexistent") is True
    assert cache.has_etherscan_proxy(1, "eth_getTransactionByHash", "0xnever_seen") is False


def test_fetch_transaction_metadata_consumes_cached_proxy(tmp_path: Path):
    """Drive fetch_transaction_metadata from the proxy cache — no API calls."""
    cache = RawResponseCache(tmp_path / "cache")
    chain_id = 1
    tx_hash = "0xdeadbeef"

    cache.write_etherscan_proxy(chain_id, "eth_getTransactionByHash", tx_hash, {
        "blockNumber": "0x11a8240",
        "input": "0xa9059cbb000000000000000000000000",
        "gasPrice": "0x4a817c800",
        "transactionIndex": "0x0",
    })
    cache.write_etherscan_proxy(chain_id, "eth_getTransactionReceipt", tx_hash, {
        "status": "0x1",
        "gasUsed": "0x5208",
        "effectiveGasPrice": "0x4a817c800",
        "cumulativeGasUsed": "0x5208",
        "blockNumber": "0x11a8240",
    })
    cache.write_etherscan_proxy(chain_id, "eth_blockNumber", "latest", "0x11a8244")

    def proxy_source(action: str, params: dict):
        key = params.get("txhash") or "latest"
        return cache.read_etherscan_proxy(chain_id, action, key)

    client = EtherscanClient(api_key="test_key", chain_id=chain_id)
    metadata = client.fetch_transaction_metadata(tx_hash, proxy_source=proxy_source)

    assert metadata is not None
    assert metadata["block_number"] == 0x11a8240
    assert metadata["gas_used"] == str(0x5208)
    assert metadata["gas_price"] == str(0x4a817c800)
    assert metadata["tx_receipt_status"] == "1"
    assert metadata["confirmations"] == 4
    assert metadata["method_id"] == "0xa9059cbb"


def test_fetch_transaction_metadata_returns_none_for_cached_null(tmp_path: Path):
    """When getTransactionByHash is cached as null, metadata fetch returns None."""
    cache = RawResponseCache(tmp_path / "cache")
    cache.write_etherscan_proxy(1, "eth_getTransactionByHash", "0xnonexistent", None)

    def proxy_source(action: str, params: dict):
        key = params.get("txhash") or "latest"
        return cache.read_etherscan_proxy(1, action, key)

    client = EtherscanClient(api_key="test_key", chain_id=1)
    assert client.fetch_transaction_metadata("0xnonexistent", proxy_source=proxy_source) is None


# ===== Solana RPC: signatures =====


def test_solana_signatures_round_trip_preserves_envelope(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")
    sig_addr = "7xKsenderaBcDeFgHiJkLmN"
    payload = {"result": [
        {"signature": "5abc", "slot": 1, "blockTime": 1, "err": None},
    ]}

    cache.write_solana_signatures_page(sig_addr, 1, payload)
    read_back = cache.read_solana_signatures_page(sig_addr, 1)

    assert read_back == payload


def test_solana_signatures_index_records_provider(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")
    sig_addr = "7xKsenderaBcDeF"
    cache.write_solana_signatures_index(sig_addr, [1, 2])

    index = cache.read_solana_signatures_index(sig_addr)
    assert index is not None
    assert index["provider"] == SOLANA_RPC_PROVIDER
    assert index["address"] == sig_addr
    assert index["pages"] == [1, 2]


def test_fetch_signatures_consumes_cached_pages(tmp_path: Path):
    """Drive _fetch_signatures from cached pages — no API calls."""
    cache = RawResponseCache(tmp_path / "cache")
    sig_addr = "7xKsender"
    page1 = {"result": [
        {"signature": "5sig1", "slot": 1, "blockTime": 1, "err": None},
        {"signature": "5sig2", "slot": 2, "blockTime": 2, "err": None},
    ]}

    cache.write_solana_signatures_page(sig_addr, 1, page1)
    cache.write_solana_signatures_index(sig_addr, [1])

    client = SolanaRpcClient(rpc_url="https://unused.example")
    sigs = client._fetch_signatures(
        sig_addr,
        signatures_source=cache.iter_solana_signatures_pages(sig_addr),
    )

    assert [s["signature"] for s in sigs] == ["5sig1", "5sig2"]


# ===== Solana RPC: per-tx blobs =====


def test_solana_transaction_round_trip_preserves_payload(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")
    tx_data = {"slot": 1, "blockTime": 1, "meta": {"fee": 5000, "err": None}}

    cache.write_solana_transaction("5sig", tx_data)
    read_back = cache.read_solana_transaction("5sig")

    assert read_back == tx_data


def test_solana_transaction_distinguishes_pruned_from_missing(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")

    cache.write_solana_transaction("5pruned", None)

    assert cache.read_solana_transaction("5pruned") is None
    assert cache.has_solana_transaction("5pruned") is True
    assert cache.has_solana_transaction("5never_seen") is False


def test_solana_transaction_path_layout(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")
    cache.write_solana_transaction("5abc", {"slot": 1})

    expected = tmp_path / "cache" / "solana_rpc" / "transactions" / "5abc.json"
    assert expected.exists()


def test_fetch_transaction_consumes_cached_blob(tmp_path: Path):
    """Drive _fetch_transaction from a cached blob via transaction_source."""
    cache = RawResponseCache(tmp_path / "cache")
    tx_data = {"slot": 42, "blockTime": 100, "meta": {"fee": 5000, "err": None}}
    cache.write_solana_transaction("5cached", tx_data)

    client = SolanaRpcClient(rpc_url="https://unused.example")
    result = client._fetch_transaction(
        "5cached",
        transaction_source=cache.read_solana_transaction,
    )

    assert result == tx_data
