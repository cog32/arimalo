#!/usr/bin/env python3
"""One-shot cleanup: drop Etherscan ``normal``/``internal`` rows that
duplicate Zerion's native-ETH ``token_transfer`` rows.

Background: a bug in ``_supplement_native_eth`` (sync.py) caused Etherscan
``txlist``/``txlistinternal`` rows to be appended even when Zerion had
already emitted a ``token_transfer`` row for the same on-chain ETH movement.
For ``txlistinternal`` (``tx_type=internal``) the trigger was contract-
mediated ETH movements (e.g. an Across bridge withdrawal); for ``txlist``
(``tx_type=normal``) the trigger was top-level ETH transfers — wallet→wallet
sends/receives and ``deposit()``-style WETH wraps where Zerion synthesises a
``token_transfer`` row with empty ``token_contract`` and Etherscan emits a
``normal`` row for the same tx.  The fix in sync.py prevents new dupes.
This script removes existing dupes from CSVs already on disk.

Match criterion: a row is a duplicate if its ``tx_type`` is ``internal`` or
``normal`` AND the same CSV contains a ``tx_type=token_transfer`` row with
the same ``tx_hash``, the same ``value``, and an empty ``token_contract``.
Matching on (tx_hash, value) — not hash alone — preserves legitimate cases
where a single tx has multiple distinct ETH movements (e.g. a main transfer
plus a small refund) and Zerion only surfaces some of them.

Usage:
    dedupe_native_eth.py [--dry-run] [VAULT_ROOT]

VAULT_ROOT defaults to ~/workspace/accountsv2.  Scans ``sources/`` recursively
for *.csv files matching the wallet-CSV schema.  Writes cleaned CSVs in place
(creating .bak siblings) unless --dry-run is given.
"""

import argparse
import csv
import sys
from pathlib import Path

csv.field_size_limit(sys.maxsize)

DEFAULT_VAULT = Path.home() / "workspace" / "accountsv2"


def find_native_eth_keys(rows: list[dict]) -> set[tuple[str, str]]:
    """Return the set of (tx_hash, value) tuples covered by a token_transfer
    row with empty token_contract (Zerion's native-ETH movements)."""
    keys: set[tuple[str, str]] = set()
    for row in rows:
        if row.get("tx_type") != "token_transfer":
            continue
        if row.get("token_contract"):
            continue
        tx_hash = row.get("tx_hash")
        value = row.get("value") or "0"
        if tx_hash:
            keys.add((tx_hash, value))
    return keys


def dedupe_csv(csv_path: Path, dry_run: bool) -> tuple[int, float]:
    """Return (rows_dropped, eth_dropped) for ``csv_path``."""
    with open(csv_path, newline="") as f:
        reader = csv.DictReader(f)
        fieldnames = reader.fieldnames
        rows = list(reader)

    if not fieldnames or "tx_type" not in fieldnames:
        return (0, 0.0)

    native_keys = find_native_eth_keys(rows)
    if not native_keys:
        return (0, 0.0)

    kept: list[dict] = []
    dropped_count = 0
    dropped_wei = 0
    for row in rows:
        if row.get("tx_type") in ("internal", "normal"):
            key = (row.get("tx_hash") or "", row.get("value") or "0")
            if key in native_keys:
                dropped_count += 1
                try:
                    dropped_wei += int(row.get("value") or "0")
                except ValueError:
                    pass
                continue
        kept.append(row)

    if dropped_count == 0:
        return (0, 0.0)

    eth_dropped = dropped_wei / 1e18

    if not dry_run:
        backup = csv_path.with_suffix(csv_path.suffix + ".bak")
        if not backup.exists():
            backup.write_bytes(csv_path.read_bytes())
        with open(csv_path, "w", newline="") as f:
            writer = csv.DictWriter(f, fieldnames=fieldnames)
            writer.writeheader()
            writer.writerows(kept)

    return (dropped_count, eth_dropped)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "vault_root",
        nargs="?",
        type=Path,
        default=DEFAULT_VAULT,
        help=f"Path to vault root (default: {DEFAULT_VAULT})",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Report what would be dropped without modifying any CSVs",
    )
    args = parser.parse_args()

    sources_dir = args.vault_root / "sources"
    if not sources_dir.is_dir():
        print(f"sources/ not found under {args.vault_root}", file=sys.stderr)
        return 2

    total_files = 0
    files_changed = 0
    total_dropped = 0
    total_eth = 0.0
    for csv_path in sorted(sources_dir.rglob("*.csv")):
        total_files += 1
        dropped, eth = dedupe_csv(csv_path, args.dry_run)
        if dropped:
            files_changed += 1
            total_dropped += dropped
            total_eth += eth
            rel = csv_path.relative_to(sources_dir)
            print(f"  {rel}: -{dropped} rows, -{eth:.4f} ETH")

    verb = "would drop" if args.dry_run else "dropped"
    print()
    print(f"Scanned {total_files} CSV files; {verb} {total_dropped} rows ({total_eth:.4f} ETH) across {files_changed} files.")
    if args.dry_run:
        print("Re-run without --dry-run to apply.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
