#!/usr/bin/env bash
# Build arimalo-covid for MCP-based UI verification.
#
# Uses an isolated CARGO_TARGET_DIR so this build does NOT share cache with
# `run_debug.sh` (which runs `tauri dev`). The two workflows compile with
# different feature flags (this one needs `tauri/custom-protocol`, tauri dev
# does not); sharing target/ would invalidate ~30 deps each switch and burn
# ~2 min per workflow change.
#
# After this script finishes, launch the printed binary via the
# tauri-automation MCP server (`launch_app` with `appPath: <printed path>`).
#
# Timing:
#   - First run (cold cache): ~5-15 min, primes ~10-30 GB into the cache dir.
#   - Warm-cache incremental rebuild after a small Rust edit: ~8 s.
#
# See SPEED.md for the benchmarking that produced this recipe.

set -euo pipefail

cd "$(dirname "$0")/.."

# Isolated target dir for the MCP build path.
export CARGO_TARGET_DIR="$HOME/.cache/arimalo-target-mcp"
mkdir -p "$CARGO_TARGET_DIR"

echo "CARGO_TARGET_DIR=$CARGO_TARGET_DIR"

# Kill any previously-launched MCP-build app instance (specifically the
# binary in this isolated target dir, so we don't disturb `tauri dev` or
# the installed app).
pkill -f "$CARGO_TARGET_DIR/debug/arimalo-covid" 2>/dev/null || true

# Frontend bundle goes into dist/, which tauri-build embeds into the binary
# via the tauri/custom-protocol feature.
echo "Building frontend (vite)..."
npm run build

# Build the Tauri app binary plus arimalo-query. Plugins (binance-prices,
# stock-prices, ...) shell out to arimalo-query, and the app resolves it next
# to its own binary — so it must live in THIS isolated target dir, not just the
# main one. Built in one invocation with the same features so the shared lib is
# reused (no dependency-rebuild churn). Still skips:
#   - 10 other arimalo-* CLI binaries (saves most of the link work)
#   - The .app/DMG bundler (saves ~60-120 s of hdiutil + codesign per build)
echo "Building arimalo-covid + arimalo-query (cargo)..."
cargo build --manifest-path src-tauri/Cargo.toml \
  --bin arimalo-covid \
  --bin arimalo-query \
  --features "webdriver tauri/custom-protocol"

BIN="$CARGO_TARGET_DIR/debug/arimalo-covid"

echo ""
echo "Built: $BIN"
echo ""
echo "Next steps:"
echo "  1. Start the WebDriver server (if not already running):"
echo "       tauri-wd --port 4444"
echo "  2. Launch via the tauri-automation MCP tool:"
echo "       mcp__tauri-automation__launch_app  appPath=$BIN"
