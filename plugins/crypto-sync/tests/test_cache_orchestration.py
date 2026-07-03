"""Tests for sync_crypto.cache_orchestration."""
from pathlib import Path
from unittest.mock import MagicMock

import pytest

from sync_crypto.cache import RawResponseCache
from sync_crypto.cache_orchestration import (
    CacheMode,
    etherscan_page_source,
    etherscan_proxy_source,
    has_etherscan_cache,
    has_solana_signatures_cache,
    has_zerion_cache,
    solana_signatures_source,
    solana_transaction_source,
    zerion_page_source,
)


# ===== presence checks =====


def test_has_zerion_cache_false_when_no_index(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")
    assert has_zerion_cache(cache, "0xabc") is False


def test_has_zerion_cache_false_when_index_empty(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")
    cache.write_zerion_index("0xabc", [])
    assert has_zerion_cache(cache, "0xabc") is False


def test_has_zerion_cache_true_when_pages_present(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")
    cache.write_zerion_page("0xabc", 1, {"data": []})
    cache.write_zerion_index("0xabc", [1])
    assert has_zerion_cache(cache, "0xabc") is True


def test_has_zerion_cache_false_when_cache_is_none():
    assert has_zerion_cache(None, "0xabc") is False


def test_has_etherscan_cache(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")
    assert has_etherscan_cache(cache, 1, "0xabc", "txlist") is False
    cache.write_etherscan_page(1, "0xabc", "txlist", 1, {"result": []})
    cache.write_etherscan_index(1, "0xabc", "txlist", [1])
    assert has_etherscan_cache(cache, 1, "0xabc", "txlist") is True
    assert has_etherscan_cache(cache, 1, "0xabc", "tokentx") is False


def test_has_solana_signatures_cache(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")
    assert has_solana_signatures_cache(cache, "5xK") is False
    cache.write_solana_signatures_page("5xK", 1, {"result": []})
    cache.write_solana_signatures_index("5xK", [1])
    assert has_solana_signatures_cache(cache, "5xK") is True


# ===== zerion_page_source =====


def test_zerion_page_source_off_uses_live(tmp_path: Path):
    live_called = [0]

    def live_factory():
        live_called[0] += 1
        yield {"page": 1}

    pages = list(zerion_page_source(None, "0xabc", CacheMode.OFF, live_factory))
    assert pages == [{"page": 1}]
    assert live_called[0] == 1


def test_zerion_page_source_read_yields_cached_pages(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")
    cache.write_zerion_page("0xabc", 1, {"page": "cached1"})
    cache.write_zerion_page("0xabc", 2, {"page": "cached2"})
    cache.write_zerion_index("0xabc", [1, 2])

    def live_factory():
        raise AssertionError("live should not be called in READ mode")

    pages = list(zerion_page_source(cache, "0xabc", CacheMode.READ, live_factory))
    assert pages == [{"page": "cached1"}, {"page": "cached2"}]


def test_zerion_page_source_write_tees_live_to_cache(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")

    def live_factory():
        yield {"page": "live1"}
        yield {"page": "live2"}

    pages = list(zerion_page_source(cache, "0xabc", CacheMode.WRITE, live_factory))
    assert pages == [{"page": "live1"}, {"page": "live2"}]

    assert cache.read_zerion_page("0xabc", 1) == {"page": "live1"}
    assert cache.read_zerion_page("0xabc", 2) == {"page": "live2"}
    index = cache.read_zerion_index("0xabc")
    assert index["pages"] == [1, 2]


def test_zerion_page_source_write_appends_to_existing_index(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")
    cache.write_zerion_page("0xabc", 1, {"page": "old1"})
    cache.write_zerion_page("0xabc", 2, {"page": "old2"})
    cache.write_zerion_index("0xabc", [1, 2])

    def live_factory():
        yield {"page": "new1"}

    list(zerion_page_source(cache, "0xabc", CacheMode.WRITE, live_factory))

    index = cache.read_zerion_index("0xabc")
    assert index["pages"] == [1, 2, 3]
    assert cache.read_zerion_page("0xabc", 1) == {"page": "old1"}  # preserved
    assert cache.read_zerion_page("0xabc", 3) == {"page": "new1"}


# ===== etherscan_page_source =====


def test_etherscan_page_source_round_trip_via_modes(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")

    def live_factory():
        yield {"result": [{"hash": "0x1"}]}
        yield {"result": [{"hash": "0x2"}]}

    # WRITE: live → cache
    pages_written = list(etherscan_page_source(
        cache, 1, "0xabc", "txlist", CacheMode.WRITE, live_factory,
    ))
    assert len(pages_written) == 2

    # READ: replay from cache (no live)
    pages_read = list(etherscan_page_source(
        cache, 1, "0xabc", "txlist", CacheMode.READ,
        lambda: (_ for _ in [pytest.fail("should not iterate live")]),
    ))
    assert pages_read == pages_written


# ===== etherscan_proxy_source =====


def test_etherscan_proxy_source_off_returns_none():
    assert etherscan_proxy_source(None, 1, CacheMode.OFF, lambda a, p: None) is None


def test_etherscan_proxy_source_read_returns_cache_reader(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")
    cache.write_etherscan_proxy(1, "eth_getTransactionByHash", "0xabc", {"x": 1})

    reader = etherscan_proxy_source(cache, 1, CacheMode.READ, lambda a, p: None)
    assert reader is not None
    assert reader("eth_getTransactionByHash", {"txhash": "0xabc"}) == {"x": 1}
    assert reader("eth_blockNumber", {}) is None  # missing key uses "latest"


def test_etherscan_proxy_source_write_tees_live_to_cache(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")
    live_calls = []

    def live_request(action: str, params: dict):
        live_calls.append((action, dict(params)))
        return {"result_for": action}

    tee = etherscan_proxy_source(cache, 1, CacheMode.WRITE, live_request)
    assert tee is not None

    result = tee("eth_getTransactionByHash", {"txhash": "0xnew"})
    assert result == {"result_for": "eth_getTransactionByHash"}
    assert live_calls == [("eth_getTransactionByHash", {"txhash": "0xnew"})]

    # Second call hits cache, no live request
    result2 = tee("eth_getTransactionByHash", {"txhash": "0xnew"})
    assert result2 == {"result_for": "eth_getTransactionByHash"}
    assert len(live_calls) == 1  # unchanged


def test_etherscan_proxy_source_write_handles_null_results(tmp_path: Path):
    """Cached null (e.g. non-existent hash) should not trigger re-fetch."""
    cache = RawResponseCache(tmp_path / "cache")
    live_calls = [0]

    def live_request(action: str, params: dict):
        live_calls[0] += 1
        return None  # tx doesn't exist

    tee = etherscan_proxy_source(cache, 1, CacheMode.WRITE, live_request)
    assert tee("eth_getTransactionByHash", {"txhash": "0xmissing"}) is None
    assert tee("eth_getTransactionByHash", {"txhash": "0xmissing"}) is None
    assert live_calls[0] == 1  # second call hit the null cache


# ===== solana_signatures_source =====


def test_solana_signatures_source_round_trip(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")

    def live_factory():
        yield {"result": [{"signature": "5sig1"}]}
        yield {"result": [{"signature": "5sig2"}]}

    written = list(solana_signatures_source(cache, "5xK", CacheMode.WRITE, live_factory))
    assert len(written) == 2

    read = list(solana_signatures_source(
        cache, "5xK", CacheMode.READ,
        lambda: (_ for _ in [pytest.fail("should not iterate live")]),
    ))
    assert read == written


# ===== solana_transaction_source =====


def test_solana_transaction_source_off_returns_none():
    assert solana_transaction_source(None, CacheMode.OFF, lambda s: None) is None


def test_solana_transaction_source_read_returns_cache_reader(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")
    cache.write_solana_transaction("5sig", {"slot": 1})

    reader = solana_transaction_source(cache, CacheMode.READ, lambda s: None)
    assert reader is not None
    assert reader("5sig") == {"slot": 1}
    assert reader("5missing") is None


def test_solana_transaction_source_write_tees_live_to_cache(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")
    live_calls = []

    def live_fetcher(signature: str):
        live_calls.append(signature)
        return {"slot": 42, "signature": signature}

    tee = solana_transaction_source(cache, CacheMode.WRITE, live_fetcher)
    assert tee is not None

    assert tee("5sig") == {"slot": 42, "signature": "5sig"}
    assert tee("5sig") == {"slot": 42, "signature": "5sig"}  # cached
    assert live_calls == ["5sig"]  # only one live call


def test_solana_transaction_source_write_caches_pruned_null(tmp_path: Path):
    cache = RawResponseCache(tmp_path / "cache")
    live_calls = [0]

    def live_fetcher(signature: str):
        live_calls[0] += 1
        return None  # pruned

    tee = solana_transaction_source(cache, CacheMode.WRITE, live_fetcher)
    assert tee("5pruned") is None
    assert tee("5pruned") is None
    assert live_calls[0] == 1


# ===== end-to-end: build → replay round-trip =====


def test_etherscan_build_then_replay_yields_same_records(tmp_path: Path):
    """Build cache via WRITE, then replay via READ — records should match."""
    from sync_crypto.clients import EtherscanClient

    cache = RawResponseCache(tmp_path / "cache")
    address = "0xwallet"
    chain_id = 1

    page1 = {
        "status": "1",
        "message": "OK",
        "result": [{
            "blockNumber": "18500000",
            "timeStamp": "1700000000",
            "hash": "0xrecord1",
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

    # WRITE: simulate live API yielding one page, tee through cache
    def live_factory():
        yield page1

    write_pages = list(etherscan_page_source(
        cache, chain_id, address, "txlist", CacheMode.WRITE, live_factory,
    ))
    assert write_pages == [page1]
    assert has_etherscan_cache(cache, chain_id, address, "txlist")

    # READ: replay from cache, normalize via client
    client = EtherscanClient(api_key="test")
    read_source = etherscan_page_source(
        cache, chain_id, address, "txlist", CacheMode.READ,
        lambda: (_ for _ in [pytest.fail("should not be called")]),
    )
    records = client._fetch_normal_transactions(address, page_source=read_source)
    assert len(records) == 1
    assert records[0].tx_hash == "0xrecord1"
