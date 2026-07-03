#!/usr/bin/env bash
# Launch Arimalo in dev mode with sources/generated from config.json (or app data fallback).
set -euo pipefail

APP_DATA="$HOME/Library/Application Support/com.cog32.arimalocovid"
CONFIG_FILE="$APP_DATA/config.json"

# Read current_root from config.json if it exists
if [ -f "$CONFIG_FILE" ]; then
  ROOT=$(python3 -c "import json,sys; c=json.load(open(sys.argv[1])); print(c.get('current_root',''))" "$CONFIG_FILE" 2>/dev/null || echo "")
else
  ROOT=""
fi

if [ -n "$ROOT" ]; then
  export ARIMALO_SOURCES_DIR="$ROOT/sources"
  export ARIMALO_GENERATED_DIR="$ROOT/generated"
else
  export ARIMALO_SOURCES_DIR="$APP_DATA/sources"
  export ARIMALO_GENERATED_DIR="$APP_DATA/generated"
fi

echo "ARIMALO_SOURCES_DIR=$ARIMALO_SOURCES_DIR"
echo "ARIMALO_GENERATED_DIR=$ARIMALO_GENERATED_DIR"

cd "$(dirname "$0")/.."

# Kill previous dev server and Tauri instances
lsof -ti:1420 2>/dev/null | xargs kill 2>/dev/null || true
pkill -f 'arimalo-covid' 2>/dev/null || true

if [ -n "$ROOT" ] && [ -d "plugins" ]; then
  echo "Installing plugins to $ROOT/plugins..."
  mkdir -p "$ROOT/plugins"
  rsync -a --delete \
    --exclude='.data/' \
    --exclude='__pycache__/' \
    --exclude='*.pyc' \
    --exclude='.pytest_cache/' \
    --exclude='.ruff_cache/' \
    --exclude='wallets.json' \
    plugins/ "$ROOT/plugins/"

  # Don't run plugins here — they have side effects (write to sources/, hit
  # external APIs). uv resolves PEP 723 deps lazily on first plugin click,
  # which is fast enough (~150ms with the hardlinked wheel cache).
  # Surface a clear warning if uv is missing so PEP-723 plugins won't fail
  # mysteriously the first time the user clicks Run.
  if ! command -v uv >/dev/null 2>&1; then
    echo "  warning: \`uv\` not on PATH — plugins with PEP 723 deps will fail."
    echo "           Install via: brew install uv  (or  curl -LsSf https://astral.sh/uv/install.sh | sh)"
  fi
fi

echo "Clearing build cache..."
rm -rf "$ARIMALO_GENERATED_DIR/.cache" "$ARIMALO_GENERATED_DIR/build-cache.json"

echo "Rebuilding pipeline..."
cargo run --manifest-path src-tauri/Cargo.toml --bin arimalo-regenerate -- \
  --sources-dir "$ARIMALO_SOURCES_DIR" \
  --generated-dir "$ARIMALO_GENERATED_DIR"

npm run tauri:dev
