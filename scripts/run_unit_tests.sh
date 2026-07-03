#!/usr/bin/env bash
set -euo pipefail

echo "Running unit tests…"

# Type-check TS sources before any test runner. Vitest transpiles without
# type-checking, so fixture/type drift slips through unless tsc gates here.
if [[ -f "tsconfig.json" ]] && [[ -f "package.json" ]] && [[ -d "node_modules" ]]; then
  echo "Type-checking with tsc --noEmit…"
  npx tsc --noEmit
fi

# Run Rust unit tests for Tauri-based repos. Note: do NOT exit afterwards —
# the TS suite (Vitest) below also gates this script, including coverage
# thresholds. Previously this block `exit 0`d after Rust tests, which
# silently skipped Vitest and made the configured coverage thresholds a
# no-op gate (see issue #8).
if [[ -f "src-tauri/Cargo.toml" ]]; then
  if command -v cargo-nextest >/dev/null 2>&1; then
    echo "Detected Rust/Tauri; running cargo nextest…"
    # nextest parallelizes test binaries and applies per-test timeouts so
    # one hung test doesn't stall the whole run. The bdd suite has
    # harness = false (custom cucumber main, not libtest-compatible), so
    # nextest can't enumerate it — exclude via filterset and run it
    # separately with `cargo test --test bdd`.
    cargo nextest run --manifest-path src-tauri/Cargo.toml --no-fail-fast \
      -E 'not binary(bdd)'
    cargo test --manifest-path src-tauri/Cargo.toml --test bdd
  else
    echo "Detected Rust/Tauri; running cargo tests (nextest not installed)…"
    cargo test --manifest-path src-tauri/Cargo.toml
  fi
fi

if [[ -f "package.json" ]]; then
  if node -e "require.resolve('vitest/package.json')" >/dev/null 2>&1; then
    echo "Detected Vitest; running with coverage thresholds…"

    if ! node -e "require.resolve('@vitest/coverage-v8/package.json')" >/dev/null 2>&1; then
      echo "ERROR: Coverage plugin '@vitest/coverage-v8' is required locally." >&2
      echo "Install it with: npm i -D @vitest/coverage-v8" >&2
      exit 1
    fi

    npx vitest run --coverage --reporter=dot

    # Enforce coverage thresholds explicitly using the JSON summary
    node - <<'NODE'
const fs = require('fs')
const path = 'coverage/coverage-summary.json'
if (!fs.existsSync(path)) {
  console.error('[coverage] coverage-summary.json not found; failing')
  process.exit(1)
}
const summary = JSON.parse(fs.readFileSync(path, 'utf8'))
// Thresholds act as a regression ratchet at current coverage floors.
// Raise these as new tests come in; do not lower without discussion.
const t = { lines: 75, functions: 75, branches: 65, statements: 73 }
const total = summary.total || {}
function pct(key) { return (total[key] && typeof total[key].pct === 'number') ? total[key].pct : 0 }
const results = {
  lines: pct('lines'),
  functions: pct('functions'),
  branches: pct('branches'),
  statements: pct('statements'),
}
let ok = true
for (const k of Object.keys(t)) {
  if (results[k] < t[k]) {
    console.error(`[coverage] ${k}: ${results[k]}% < threshold ${t[k]}%`)
    ok = false
  }
}
if (!ok) process.exit(1)
console.log('[coverage] thresholds met:', results)
NODE

    exit 0
  fi

  if grep -q '"test"' package.json; then
    echo "No Vitest detected; running npm test…"
    npm test
    exit 0
  fi
fi

echo "No unit test runner detected; skipping."
