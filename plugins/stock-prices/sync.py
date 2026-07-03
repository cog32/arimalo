#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = ["yfinance>=0.2"]
# ///
"""Stock Yahoo Finance Price Sync Plugin for Arimalo.

Prices the equities you actually hold, across every market, in one plugin.

For each configured market (``account_prefix | yahoo_suffix | currency``) it:

  1. Discovers the commodities posted under ``account_prefix`` via arimalo-query.
  2. Builds the Yahoo ticker ``<COMMODITY><yahoo_suffix>`` (or a ticker_override).
  3. Fetches daily closes and merges them into ``sources/_prices/<COMMODITY>.txt``.

Defaults cover CommSec (``...:commsec:`` -> ``.AX`` -> AUD) and Interactive
Brokers (``...:ibkr:`` -> no suffix -> USD). US prices land in USD; the report
converts them to the base currency via the existing fiat USD->AUD chain.

Incremental: a commodity that already has a price file is fetched only from its
last recorded date forward, and the new closes are merged in. This makes the
plugin safe to run daily — an up-to-date file fetches nothing new and is left
untouched. (Re-fetching a date overwrites it, so yfinance close revisions are
picked up.)

Non-equity commodities under a broker prefix (bond CUSIPs, option symbols) do
not look like tickers and are skipped without a network call. Fiat commodities
(the USD/AUD cash legs) are excluded from discovery.

Output (P-directive format)::

    P <YYYY-MM-DD>T00:00:00.000000Z <COMMODITY> <PRICE> <CURRENCY>
"""

import json
import os
import re
import subprocess
import sys

# Commodities that are currencies, not equities — never fetch these as tickers.
FIAT = {"AUD", "USD", "EUR", "GBP", "NZD", "JPY", "CHF", "CAD", "HKD", "SGD", "CNY"}

# A fetchable equity ticker: starts with a letter, <=10 chars, letters/digits
# plus '.'/'-'. Excludes bond CUSIPs (start with a digit), option symbols
# (too long), and anything with a '/' (which would also break the filename).
TICKER_RE = re.compile(r"^[A-Za-z][A-Za-z0-9.\-]{0,9}$")


def parse_kv_overrides(raw):
    """Parse 'A=X,B=Y' into {'A': 'X', 'B': 'Y'}; empty/blank input -> {}."""
    out = {}
    for pair in (raw or "").split(","):
        pair = pair.strip()
        if "=" in pair:
            k, v = pair.split("=", 1)
            out[k.strip()] = v.strip()
    return out


def parse_markets(raw):
    """Parse 'prefix|suffix|ccy;prefix2|suffix2|ccy2' into a list of dicts.

    Order is preserved: earlier markets win when the same commodity is held in
    more than one (the first prices it; later ones report a collision).
    """
    markets = []
    for entry in (raw or "").split(";"):
        entry = entry.strip()
        if not entry:
            continue
        parts = entry.split("|")
        if len(parts) != 3:
            raise ValueError(
                f"bad market '{entry}': expected 'account_prefix|yahoo_suffix|currency'"
            )
        prefix, suffix, currency = (p.strip() for p in parts)
        if not prefix or not currency:
            raise ValueError(f"bad market '{entry}': prefix and currency are required")
        markets.append({"prefix": prefix, "suffix": suffix, "currency": currency})
    return markets


def run_arimalo_query(query_bin):
    """Return the parsed `arimalo-query --format json` payload for the vault."""
    result = subprocess.run(
        [query_bin, "--format", "json"],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        raise RuntimeError(f"arimalo-query failed: {result.stderr.strip()}")
    return json.loads(result.stdout)


def discover_commodities(data, account_prefix):
    """Sorted unique non-fiat commodities posted to accounts under a prefix."""
    found = set()
    for txn in data.get("transactions", []):
        for posting in txn.get("postings", []):
            account = posting.get("account", "") or ""
            commodity = (posting.get("commodity") or "").strip()
            if not commodity or commodity.upper() in FIAT:
                continue
            if account.startswith(account_prefix):
                found.add(commodity)
    return sorted(found)


def fetch_close(ticker, period, start=None):
    """Return list of (YYYY-MM-DD, close_price) for a Yahoo ticker.

    When ``start`` (a YYYY-MM-DD string) is given, fetch only from that date
    forward and treat an empty result as "already up to date" (returns ``[]``,
    not an error). Without ``start`` an empty result is an error (the ticker is
    wrong or delisted).

    yfinance is imported lazily so the rest of the module (discovery, parsing,
    file writing) is importable and unit-testable without the dependency.
    """
    import yfinance as yf

    if start is not None:
        data = yf.download(ticker, start=start, progress=False, auto_adjust=False)
    else:
        data = yf.download(ticker, period=period, progress=False, auto_adjust=False)
    if data is None or data.empty:
        if start is not None:
            return []  # up to date — nothing new since `start`
        raise RuntimeError(f"yfinance returned no data for {ticker}")

    close = data["Close"]
    if hasattr(close, "iloc") and getattr(close, "ndim", 1) == 2:
        close = close.iloc[:, 0]

    out = []
    for ts, price in close.dropna().items():
        out.append((ts.strftime("%Y-%m-%d"), float(price)))
    return out


def load_existing_prices(filepath):
    """Map YYYY-MM-DD -> price string from an existing P-directive file.

    Stock price lines carry a full ISO timestamp (``P <date>T00:00:00.000000Z
    <COMMODITY> <price> <ccy>``); we key on the date component only.
    """
    existing = {}
    if not os.path.exists(filepath):
        return existing
    with open(filepath) as f:
        for line in f:
            line = line.strip()
            if not line.startswith("P "):
                continue
            parts = line.split()
            if len(parts) >= 5:
                date = parts[1].split("T")[0]
                existing[date] = parts[3]
    return existing


def write_prices(filepath, commodity, prices, currency, existing):
    """Merge new prices into existing and rewrite the file.

    Existing dates keep their stored value unless re-fetched; a re-fetched date
    is overwritten (picking up yfinance revisions). Returns the count of dates
    that were not previously present.
    """
    merged = dict(existing)
    new_count = 0
    for date_str, price in prices:
        if date_str not in merged:
            new_count += 1
        merged[date_str] = f"{price:.4f}"

    os.makedirs(os.path.dirname(filepath), exist_ok=True)
    with open(filepath, "w") as f:
        for date_str in sorted(merged.keys()):
            f.write(
                f"P {date_str}T00:00:00.000000Z {commodity} {merged[date_str]} {currency}\n"
            )
    return new_count


def main():
    ctx = json.load(sys.stdin)
    config = ctx.get("config", {})
    sources_dir = ctx["sources_dir"]
    query_bin = ctx.get("bin", {}).get("arimalo_query")

    if not query_bin:
        print(
            json.dumps(
                {
                    "files_written": [],
                    "records_fetched": 0,
                    "warnings": ["arimalo-query binary not provided (ctx.bin.arimalo_query)"],
                }
            )
        )
        sys.exit(1)

    markets = parse_markets(
        config.get(
            "markets",
            "assets:equity:broker:commsec:|.AX|AUD;assets:equity:broker:ibkr:||USD",
        )
    )
    period = config.get("period", "max")
    overrides = parse_kv_overrides(config.get("ticker_overrides", ""))

    prices_dir = os.path.join(sources_dir, "_prices")
    data = run_arimalo_query(query_bin)

    files_written = []
    total_new = 0
    warnings = []          # real fetch failures only — these drive the exit code
    skipped_nonticker = []  # informational (bonds/CUSIPs) — logged to stderr
    ok = 0                  # commodities priced or already up to date
    seen = {}  # commodity -> market prefix that first claimed it

    for market in markets:
        prefix = market["prefix"]
        suffix = market["suffix"]
        currency = market["currency"]
        commodities = discover_commodities(data, prefix)
        print(
            f"Discovered {len(commodities)} commodities under {prefix}",
            file=sys.stderr,
        )

        for commodity in commodities:
            if commodity in seen:
                # Held under two markets — priced once, under the first. This is
                # informational, not a failure.
                print(
                    f"{commodity}: held under both {seen[commodity]} and {prefix}; "
                    f"priced once under {seen[commodity]}",
                    file=sys.stderr,
                )
                continue
            seen[commodity] = prefix

            filepath = os.path.join(prices_dir, f"{commodity}.txt")
            existing = load_existing_prices(filepath)

            override = overrides.get(commodity)
            if override is None and not TICKER_RE.match(commodity):
                # Bond CUSIP / option symbol / non-ticker — not on Yahoo. A
                # manually-maintained price file counts as up to date; otherwise
                # it's an informational skip, not a failure.
                if existing:
                    ok += 1
                else:
                    skipped_nonticker.append(commodity)
                continue
            ticker = override if override is not None else f"{commodity}{suffix}"

            # Incremental: fetch only from the last recorded date forward.
            start = max(existing) if existing else None
            try:
                print(f"Fetching {ticker} for {commodity} (from {start or 'start'})...", file=sys.stderr)
                prices = fetch_close(ticker, period, start=start)
            except Exception as e:
                warnings.append(f"{commodity} ({ticker}): {e}")
                continue
            if not prices:
                if existing:
                    ok += 1  # already up to date — incremental fetch found nothing
                else:
                    warnings.append(f"{commodity} ({ticker}): no data returned")
                continue

            ok += 1
            new_count = write_prices(filepath, commodity, prices, currency, existing)
            if new_count > 0:
                total_new += new_count
                files_written.append(f"_prices/{commodity}.txt")
                print(f"  Wrote {new_count} new prices to {commodity}.txt", file=sys.stderr)

    if skipped_nonticker:
        # Informational only (these aren't tickers Yahoo can price) — logged to
        # stderr, kept out of `warnings` and the exit code.
        print(
            f"Skipped {len(skipped_nonticker)} non-ticker commodity(ies) "
            f"(bonds/options/CUSIPs): " + ", ".join(skipped_nonticker),
            file=sys.stderr,
        )

    print(
        json.dumps(
            {
                "files_written": files_written,
                "records_fetched": total_new,
                "warnings": warnings,
            }
        )
    )

    # `warnings` holds only real fetch failures. Exit 1 only when nothing could
    # be priced at all; exit 2 on partial failure; exit 0 otherwise — including a
    # fully up-to-date run, or one whose only notes are skipped bonds/CUSIPs.
    if warnings and ok == 0 and not files_written:
        sys.exit(1)
    elif warnings:
        sys.exit(2)


if __name__ == "__main__":
    main()
