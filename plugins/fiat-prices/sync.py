#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = ["yfinance>=0.2"]
# ///
"""Fiat Price Sync Plugin for Arimalo.

Fetches daily fiat/USD historical exchange rates from Yahoo Finance via
the yfinance library and writes P-directive files to
sources/_prices/{CURRENCY}.txt.

Mirrors the pattern from edwin-computer-use's
external/edwin/global_liquidity.py, where yfinance is used with tickers
of the form '{FROM}{TO}=X' (e.g. 'AUDUSD=X', 'EURUSD=X') to obtain
historical FX closes.

Output format matches the rest of the arimalo pricing pipeline:
    P <YYYY-MM-DD> <CURRENCY> <PRICE> USD

Dependencies are declared in PEP 723 inline metadata above; the runner
will use `uv run` to provision an ephemeral venv automatically.
"""

import json
import os
import sys

import yfinance as yf


def parse_csv_list(raw):
    return [s.strip().upper() for s in raw.split(",") if s.strip()]


def fetch_fx_close(currency, period):
    """Return list of (YYYY-MM-DD, close_price_in_usd) for the given fiat currency.

    Uses Yahoo's '{FROM}USD=X' ticker, which quotes 1 unit of the FROM
    currency in USD — the exact value we want for a P-directive.
    """
    ticker = f"{currency}USD=X"
    data = yf.download(ticker, period=period, progress=False, auto_adjust=False)
    if data is None or data.empty:
        raise RuntimeError(f"yfinance returned no data for {ticker}")

    close = data["Close"]
    if hasattr(close, "iloc") and getattr(close, "ndim", 1) == 2:
        close = close.iloc[:, 0]

    out = []
    for ts, price in close.dropna().items():
        date_str = ts.strftime("%Y-%m-%d")
        out.append((date_str, float(price)))
    return out


def load_existing_prices(filepath):
    existing = {}
    if not os.path.exists(filepath):
        return existing
    with open(filepath) as f:
        for line in f:
            line = line.strip()
            if not line or not line.startswith("P "):
                continue
            parts = line.split()
            if len(parts) >= 5:
                existing[parts[1]] = parts[3]
    return existing


def write_prices(filepath, currency, prices, existing):
    """Merge new prices with existing and rewrite the file. Returns count of new entries."""
    merged = dict(existing)
    new_count = 0
    for date_str, price in prices:
        formatted = f"{price:.6f}"
        if date_str not in merged:
            new_count += 1
        merged[date_str] = formatted

    os.makedirs(os.path.dirname(filepath), exist_ok=True)
    with open(filepath, "w") as f:
        for date_str in sorted(merged.keys()):
            f.write(f"P {date_str} {currency} {merged[date_str]} USD\n")
    return new_count


def main():
    ctx = json.load(sys.stdin)
    config = ctx.get("config", {})
    sources_dir = ctx["sources_dir"]

    currencies = parse_csv_list(config.get("currencies", "AUD,EUR,GBP"))
    period = config.get("period", "max")

    files_written = []
    total_fetched = 0
    warnings = []

    prices_dir = os.path.join(sources_dir, "_prices")

    for currency in currencies:
        if currency == "USD":
            continue  # USD is the quote currency
        try:
            print(f"Fetching {currency}USD=X...", file=sys.stderr)
            prices = fetch_fx_close(currency, period)
        except Exception as e:
            warnings.append(f"{currency}: {e}")
            continue

        filepath = os.path.join(prices_dir, f"{currency}.txt")
        existing = load_existing_prices(filepath)
        new_count = write_prices(filepath, currency, prices, existing)
        total_fetched += len(prices)

        if new_count > 0:
            files_written.append(f"_prices/{currency}.txt")
            print(f"  Wrote {new_count} new prices to {currency}.txt", file=sys.stderr)

    result = {
        "files_written": files_written,
        "records_fetched": total_fetched,
        "warnings": warnings,
    }
    print(json.dumps(result))

    if warnings and not files_written:
        sys.exit(1)
    elif warnings:
        sys.exit(2)


if __name__ == "__main__":
    main()
