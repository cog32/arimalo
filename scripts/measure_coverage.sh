#!/usr/bin/env bash
# Measures line coverage for both Rust and TypeScript and prints a single JSON
# summary to stdout: {"rust_lines": 69.14, "ts_lines": 70.12}.
#
# Both numbers exclude test code from the metric while letting test execution
# count toward production-code coverage. For Rust, that means BDD scenarios
# in src-tauri/tests/bdd.rs contribute their coverage of src/ but the bdd.rs
# file itself is not in the denominator.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

# Rust toolchain via rustup but cargo via homebrew → llvm tools must be located
# explicitly. Allow override via env.
LLVM_BIN_DIR="${LLVM_BIN_DIR:-$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/aarch64-apple-darwin/bin}"
export LLVM_COV="${LLVM_COV:-$LLVM_BIN_DIR/llvm-cov}"
export LLVM_PROFDATA="${LLVM_PROFDATA:-$LLVM_BIN_DIR/llvm-profdata}"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

# --- Rust ---
# Use --no-fail-fast so a single flaky integration test doesn't void the
# coverage report — coverage is collected for everything that did run.
# Coverage is collected in two passes and merged: nextest covers libtest +
# integration tests in parallel; bdd has harness=false so nextest can't
# run it — collect its coverage via plain `cargo llvm-cov --test bdd`.
RUST_PCT="null"
if command -v cargo-llvm-cov >/dev/null 2>&1 \
  && command -v cargo-nextest >/dev/null 2>&1 \
  && [ -f "src-tauri/Cargo.toml" ]; then
  (
    cd src-tauri
    cargo llvm-cov clean --workspace --quiet >/dev/null 2>&1 || true
    cargo llvm-cov nextest --no-report --no-fail-fast --quiet \
      -E 'not binary(bdd)' >/dev/null 2>&1 || true
    cargo llvm-cov --no-clean --no-report --test bdd --no-fail-fast --quiet >/dev/null 2>&1 || true
    cargo llvm-cov report \
      --ignore-filename-regex '(tests/|gen/)' \
      --json --output-path "$TMP_DIR/rust.json" \
      --quiet >/dev/null 2>&1
  ) || true
  if [ -s "$TMP_DIR/rust.json" ]; then
    RUST_PCT=$(python3 -c "
import json
d = json.load(open('$TMP_DIR/rust.json'))
totals = d['data'][0]['totals']
print(round(totals['lines']['percent'], 2))
")
  fi
elif command -v cargo-llvm-cov >/dev/null 2>&1 && [ -f "src-tauri/Cargo.toml" ]; then
  # Fallback: nextest not available, single-process run covers everything.
  (cd src-tauri && cargo llvm-cov --tests --no-fail-fast \
      --ignore-filename-regex '(tests/|gen/)' \
      --json --output-path "$TMP_DIR/rust.json" \
      --quiet >/dev/null 2>&1) || true
  if [ -s "$TMP_DIR/rust.json" ]; then
    RUST_PCT=$(python3 -c "
import json
d = json.load(open('$TMP_DIR/rust.json'))
totals = d['data'][0]['totals']
print(round(totals['lines']['percent'], 2))
")
  fi
fi

# --- TypeScript ---
TS_PCT="null"
if [ -f "package.json" ] && [ -d "node_modules/@vitest/coverage-v8" ]; then
  npx vitest run --coverage --reporter=dot >/dev/null 2>&1 || true
  if [ -f "coverage/coverage-summary.json" ]; then
    TS_PCT=$(python3 -c "
import json
d = json.load(open('coverage/coverage-summary.json'))
print(round(d['total']['lines']['pct'], 2))
")
  fi
fi

printf '{"rust_lines": %s, "ts_lines": %s}\n' "$RUST_PCT" "$TS_PCT"
