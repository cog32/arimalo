#!/usr/bin/env bash
set -euo pipefail

echo "Setting up Appium mac2 driver…"

if ! command -v xcodebuild >/dev/null 2>&1; then
  echo "ERROR: Xcode is required (xcodebuild not found)." >&2
  exit 1
fi

if ! command -v npx >/dev/null 2>&1; then
  echo "ERROR: Node.js is required (npx not found)." >&2
  exit 1
fi

if ! node -e "require.resolve('appium')" >/dev/null 2>&1; then
  echo "ERROR: appium is not installed. Run: npm install" >&2
  exit 1
fi

echo "Installing mac2 driver (Appium)…"
npx appium driver install mac2

echo "Running driver doctor…"
npx appium driver doctor mac2 || true

echo "Done."

