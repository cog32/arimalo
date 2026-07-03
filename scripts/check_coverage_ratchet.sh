#!/usr/bin/env bash
# Coverage ratchet: re-measures Rust + TS line coverage and compares against
# the baseline in .coverage-baseline.json.
#
# Rules:
#   - If a metric is at 100%, it must stay at 100% (no regression allowed).
#   - If a metric is below 100%, the new value must be >= the baseline.
#     Regressions below 100% are rejected.
#   - On improvement, the baseline is updated and re-staged so the new
#     baseline lands in the same commit.
#
# Skip the entire check by setting `SKIP_COVERAGE_RATCHET=1`.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

if [ "${SKIP_COVERAGE_RATCHET:-0}" = "1" ]; then
  echo "[ratchet] SKIP_COVERAGE_RATCHET=1 — skipping coverage check."
  exit 0
fi

BASELINE_FILE=".coverage-baseline.json"

echo "[ratchet] Measuring coverage (this takes ~30s on warm caches)..."
NEW_JSON="$(./scripts/measure_coverage.sh)"
echo "[ratchet] Measured: $NEW_JSON"

if [ ! -f "$BASELINE_FILE" ]; then
  echo "[ratchet] No baseline yet — writing initial baseline:"
  echo "$NEW_JSON" > "$BASELINE_FILE"
  git add "$BASELINE_FILE"
  echo "[ratchet] Initial baseline staged."
  exit 0
fi

OLD_JSON="$(cat "$BASELINE_FILE")"

python3 - <<PY
import json, subprocess, sys

old = json.loads('''$OLD_JSON''')
new = json.loads('''$NEW_JSON''')

regressed = []
improved = []

def check(name, old_v, new_v):
    if old_v is None or new_v is None:
        return  # tool wasn't available — silently skip
    if old_v >= 100.0:
        if new_v < 100.0:
            regressed.append(f"{name}: was 100.00%, now {new_v:.2f}% — must stay at 100%")
        return
    if new_v + 1e-6 < old_v:
        regressed.append(f"{name}: dropped {old_v:.2f}% → {new_v:.2f}% (delta -{old_v - new_v:.2f}pp)")
    elif new_v > old_v + 1e-6:
        improved.append(f"{name}: {old_v:.2f}% → {new_v:.2f}% (+{new_v - old_v:.2f}pp)")

check("rust_lines", old.get("rust_lines"), new.get("rust_lines"))
check("ts_lines", old.get("ts_lines"), new.get("ts_lines"))

if regressed:
    print("[ratchet] FAIL: coverage regressed:")
    for r in regressed:
        print(f"  - {r}")
    print("\nFix by adding tests, or set SKIP_COVERAGE_RATCHET=1 to override (not recommended).")
    sys.exit(1)

if improved:
    print("[ratchet] Coverage improved — updating baseline:")
    for i in improved:
        print(f"  + {i}")
    with open("$BASELINE_FILE", "w") as f:
        json.dump(new, f, indent=2)
        f.write("\n")
    subprocess.run(["git", "add", "$BASELINE_FILE"], check=True)
else:
    print("[ratchet] OK — coverage held steady.")
PY
