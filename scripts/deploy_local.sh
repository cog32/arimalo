#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "Building local deployment…"

if [[ ! -d "node_modules" ]]; then
  echo "Installing JS dependencies…"
  npm install
fi

echo "Building Tauri bundle…"
npm run -s tauri:build

OS_NAME="$(uname -s)"
if [[ "$OS_NAME" == "Darwin" ]]; then
  PRODUCT_NAME="$(node -p "require('./src-tauri/tauri.conf.json').productName")"
  APP_PATH="src-tauri/target/release/bundle/macos/${PRODUCT_NAME}.app"

  if [[ ! -d "$APP_PATH" ]]; then
    echo "ERROR: Expected app bundle not found: $APP_PATH" >&2
    exit 1
  fi

  DEST="/Applications/${PRODUCT_NAME}.app"

  echo "Installing to ${DEST}…"
  if [[ -d "$DEST" ]]; then
    rm -rf "$DEST"
  fi
  cp -R "$APP_PATH" "$DEST"

  echo "Done: installed to $DEST"
else
  echo "Done: bundle created under src-tauri/target/release/bundle/"
  echo "(Manual install required on this platform)"
fi
