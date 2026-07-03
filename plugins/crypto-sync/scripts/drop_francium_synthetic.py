#!/usr/bin/env python3
"""One-shot cleanup: drop Zerion-Solana enrichment rows that synthesised
"position" sends/receives for Francium vault movements.

Background: ``_append_francium_position_records`` (zerion.py) emitted
synthetic rows for token movements inside a Francium yield vault even when
the wallet didn't own the destination/source ATA — the vault is
protocol-controlled and the wallet only holds the position NFT, not the
underlying. Those rows pollute reconciliations because the underlying never
hit the wallet. The enrichment helper has been removed; this script drops
the rows it already wrote into vault CSVs.

Match criterion: ``record_id`` (first column) contains ``:francium:``.

Usage:
    drop_francium_synthetic.py [--dry-run] [VAULT_ROOT]

VAULT_ROOT defaults to ~/workspace/accountsv2.  Scans ``sources/`` recursively
for *.csv files; writes cleaned CSVs in place (creating .bak siblings) unless
--dry-run is given.
"""

import argparse
import csv
import sys
from pathlib import Path

csv.field_size_limit(sys.maxsize)

DEFAULT_VAULT = Path.home() / "workspace" / "accountsv2"
MARKER = ":francium:"


def drop_francium(csv_path: Path, dry_run: bool) -> int:
    with open(csv_path, newline="") as f:
        reader = csv.reader(f)
        try:
            header = next(reader)
        except StopIteration:
            return 0
        rows = list(reader)

    kept = [row for row in rows if not (row and MARKER in row[0])]
    dropped = len(rows) - len(kept)
    if dropped == 0:
        return 0

    if not dry_run:
        backup = csv_path.with_suffix(csv_path.suffix + ".bak")
        if not backup.exists():
            backup.write_bytes(csv_path.read_bytes())
        with open(csv_path, "w", newline="") as f:
            writer = csv.writer(f)
            writer.writerow(header)
            writer.writerows(kept)

    return dropped


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
    for csv_path in sorted(sources_dir.rglob("*.csv")):
        total_files += 1
        dropped = drop_francium(csv_path, args.dry_run)
        if dropped:
            files_changed += 1
            total_dropped += dropped
            rel = csv_path.relative_to(sources_dir)
            print(f"  {rel}: -{dropped} rows")

    verb = "would drop" if args.dry_run else "dropped"
    print()
    print(f"Scanned {total_files} CSV files; {verb} {total_dropped} rows across {files_changed} files.")
    if args.dry_run:
        print("Re-run without --dry-run to apply.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
