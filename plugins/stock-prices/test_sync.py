"""Unit tests for the stock-prices plugin. No network: fetch_close is patched
and arimalo-query is a fake script that prints canned JSON."""

import io
import json
import os
import stat
import sys

import pytest

import sync


def test_parse_kv_overrides():
    assert sync.parse_kv_overrides("BRKB=BRK-B, GOOGL=GOOG") == {
        "BRKB": "BRK-B",
        "GOOGL": "GOOG",
    }
    assert sync.parse_kv_overrides("") == {}
    assert sync.parse_kv_overrides("   ") == {}


def test_parse_markets_ok():
    markets = sync.parse_markets(
        "assets:equity:broker:commsec:|.AX|AUD;assets:equity:broker:ibkr:||USD"
    )
    assert markets == [
        {"prefix": "assets:equity:broker:commsec:", "suffix": ".AX", "currency": "AUD"},
        {"prefix": "assets:equity:broker:ibkr:", "suffix": "", "currency": "USD"},
    ]


def test_parse_markets_rejects_malformed():
    with pytest.raises(ValueError):
        sync.parse_markets("only|two")
    with pytest.raises(ValueError):
        sync.parse_markets("|.AX|AUD")  # empty prefix


def test_discover_filters_by_prefix_and_excludes_fiat():
    data = {
        "transactions": [
            {
                "postings": [
                    {"account": "assets:equity:broker:ibkr:personal", "commodity": "AMZN"},
                    {"account": "assets:equity:broker:ibkr:personal:cash", "commodity": "USD"},
                    {"account": "assets:equity:broker:commsec:personal", "commodity": "BHP"},
                    {"account": "assets:crypto:exchange:kraken", "commodity": "BTC"},
                ]
            }
        ]
    }
    assert sync.discover_commodities(data, "assets:equity:broker:ibkr:") == ["AMZN"]
    assert sync.discover_commodities(data, "assets:equity:broker:commsec:") == ["BHP"]


def test_ticker_regex_accepts_equities_rejects_cusips_options():
    good = ["AMZN", "GOOGL", "BRK-B", "AGX1", "GSIO40", "BAYNd"]
    bad = ["912796CR8", "QQQ270319P00635000", "T01/808/31/23", "JPM37/809/10/24"]
    assert all(sync.TICKER_RE.match(t) for t in good)
    assert not any(sync.TICKER_RE.match(t) for t in bad)


def test_load_existing_prices_parses_full_iso_timestamps(tmp_path):
    fp = tmp_path / "CBA.txt"
    fp.write_text(
        "P 2026-06-29T00:00:00.000000Z CBA 95.5000 AUD\n"
        "P 2026-06-30T00:00:00.000000Z CBA 96.0000 AUD\n"
    )
    assert sync.load_existing_prices(str(fp)) == {
        "2026-06-29": "95.5000",
        "2026-06-30": "96.0000",
    }
    assert sync.load_existing_prices(str(tmp_path / "missing.txt")) == {}


def test_write_prices_merges_and_preserves_format(tmp_path):
    fp = tmp_path / "AMZN.txt"
    existing = {"2026-06-29": "40.0000"}
    n = sync.write_prices(str(fp), "AMZN", [("2026-06-30", 219.123456)], "USD", existing)
    assert n == 1
    assert fp.read_text().splitlines() == [
        "P 2026-06-29T00:00:00.000000Z AMZN 40.0000 USD",
        "P 2026-06-30T00:00:00.000000Z AMZN 219.1235 USD",
    ]


def test_write_prices_counts_only_new_dates(tmp_path):
    fp = tmp_path / "AMZN.txt"
    existing = {"2026-06-30": "219.0000"}
    # Re-fetching the same date overwrites it but is not counted as new.
    n = sync.write_prices(str(fp), "AMZN", [("2026-06-30", 220.0)], "USD", existing)
    assert n == 0
    assert fp.read_text() == "P 2026-06-30T00:00:00.000000Z AMZN 220.0000 USD\n"


def _fake_query_bin(tmp_path, payload):
    """Create an executable script that prints `payload` as JSON on stdout."""
    script = tmp_path / "arimalo-query-fake"
    script.write_text(
        "#!/usr/bin/env python3\nimport sys\n"
        f"sys.stdout.write({json.dumps(json.dumps(payload))})\n"
    )
    script.chmod(script.stat().st_mode | stat.S_IEXEC)
    return str(script)


def test_main_end_to_end_incremental(tmp_path, monkeypatch, capsys):
    payload = {
        "transactions": [
            {
                "postings": [
                    {"account": "assets:equity:broker:ibkr:personal", "commodity": "AMZN"},
                    {"account": "assets:equity:broker:ibkr:personal", "commodity": "912796CR8"},
                    {"account": "assets:equity:broker:commsec:personal", "commodity": "BHP"},
                    {"account": "assets:equity:broker:commsec:personal", "commodity": "AMZN"},
                ]
            }
        ]
    }
    query_bin = _fake_query_bin(tmp_path, payload)
    sources_dir = tmp_path / "sources"
    os.makedirs(sources_dir / "_prices")
    # BHP already has prices through 2026-06-29 -> fetch incrementally from
    # there and MERGE (not skip-if-exists, not wholesale overwrite).
    (sources_dir / "_prices" / "BHP.txt").write_text(
        "P 2026-06-29T00:00:00.000000Z BHP 40.0000 AUD\n"
    )

    captured = {}

    def fake_fetch(ticker, period, start=None):
        captured[ticker] = start
        return [("2026-06-30", 100.0)]

    monkeypatch.setattr(sync, "fetch_close", fake_fetch)

    ctx = {
        "sources_dir": str(sources_dir),
        "config": {},  # use built-in default markets
        "bin": {"arimalo_query": query_bin},
    }
    monkeypatch.setattr(sys, "stdin", io.StringIO(json.dumps(ctx)))

    # AMZN + BHP both price fine; the CUSIP and the AMZN collision are
    # informational notices, not failures -> clean exit 0 (no SystemExit).
    sync.main()
    cap = capsys.readouterr()
    out = json.loads(cap.out)
    # AMZN has no file -> full fetch (start None); BHP -> incremental from its
    # last date. AMZN priced under commsec (.AX) which claims it first.
    assert captured == {"AMZN.AX": None, "BHP.AX": "2026-06-29"}
    assert set(out["files_written"]) == {"_prices/AMZN.txt", "_prices/BHP.txt"}
    assert out["warnings"] == []  # no real fetch errors
    # BHP merged: old line preserved + new date appended.
    bhp = (sources_dir / "_prices" / "BHP.txt").read_text()
    assert "P 2026-06-29T00:00:00.000000Z BHP 40.0000 AUD" in bhp
    assert "P 2026-06-30T00:00:00.000000Z BHP 100.0000 AUD" in bhp
    # CUSIP + collision are reported on stderr (informational), not in warnings.
    assert "912796CR8" in cap.err
    assert "both" in cap.err and "AMZN" in cap.err
    # AMZN written in AUD because commsec (.AX) claimed it first.
    assert (sources_dir / "_prices" / "AMZN.txt").read_text().endswith(" AUD\n")


def test_delisted_ticker_is_partial_not_failure(tmp_path, monkeypatch, capsys):
    """One ticker fails to fetch but another succeeds -> exit 2 (partial),
    not exit 1, and not a clean run either."""
    payload = {"transactions": [{"postings": [
        {"account": "assets:equity:broker:commsec:personal", "commodity": "BHP"},
        {"account": "assets:equity:broker:commsec:personal", "commodity": "AED"},
    ]}]}
    query_bin = _fake_query_bin(tmp_path, payload)
    sources_dir = tmp_path / "sources"
    os.makedirs(sources_dir / "_prices")

    def fake_fetch(ticker, period, start=None):
        if ticker == "AED.AX":
            raise RuntimeError("possibly delisted; no price data found")
        return [("2026-06-30", 40.0)]

    monkeypatch.setattr(sync, "fetch_close", fake_fetch)
    ctx = {"sources_dir": str(sources_dir), "config": {}, "bin": {"arimalo_query": query_bin}}
    monkeypatch.setattr(sys, "stdin", io.StringIO(json.dumps(ctx)))

    with pytest.raises(SystemExit) as exc:
        sync.main()
    assert exc.value.code == 2
    out = json.loads(capsys.readouterr().out)
    assert out["files_written"] == ["_prices/BHP.txt"]
    assert any("AED" in w for w in out["warnings"])


def test_all_tickers_unpricable_is_failure(tmp_path, monkeypatch, capsys):
    """When nothing can be priced at all -> exit 1."""
    payload = {"transactions": [{"postings": [
        {"account": "assets:equity:broker:commsec:personal", "commodity": "AED"},
    ]}]}
    query_bin = _fake_query_bin(tmp_path, payload)
    sources_dir = tmp_path / "sources"
    os.makedirs(sources_dir / "_prices")
    monkeypatch.setattr(sync, "fetch_close",
                        lambda t, p, start=None: (_ for _ in ()).throw(RuntimeError("delisted")))
    ctx = {"sources_dir": str(sources_dir), "config": {}, "bin": {"arimalo_query": query_bin}}
    monkeypatch.setattr(sys, "stdin", io.StringIO(json.dumps(ctx)))

    with pytest.raises(SystemExit) as exc:
        sync.main()
    assert exc.value.code == 1


def test_incremental_empty_fetch_is_not_a_warning(tmp_path, monkeypatch, capsys):
    """An up-to-date file whose incremental fetch returns nothing is a clean
    no-op (exit 0), not a warning — so daily runs don't churn on weekends."""
    payload = {
        "transactions": [
            {"postings": [{"account": "assets:equity:broker:commsec:personal", "commodity": "BHP"}]}
        ]
    }
    query_bin = _fake_query_bin(tmp_path, payload)
    sources_dir = tmp_path / "sources"
    os.makedirs(sources_dir / "_prices")
    original = "P 2026-06-30T00:00:00.000000Z BHP 40.0000 AUD\n"
    (sources_dir / "_prices" / "BHP.txt").write_text(original)

    monkeypatch.setattr(sync, "fetch_close", lambda ticker, period, start=None: [])

    ctx = {"sources_dir": str(sources_dir), "config": {}, "bin": {"arimalo_query": query_bin}}
    monkeypatch.setattr(sys, "stdin", io.StringIO(json.dumps(ctx)))

    # No warnings, nothing written -> falls through, no SystemExit (exit 0).
    sync.main()
    out = json.loads(capsys.readouterr().out)
    assert out["files_written"] == []
    assert out["warnings"] == []
    # File left byte-for-byte untouched.
    assert (sources_dir / "_prices" / "BHP.txt").read_text() == original
