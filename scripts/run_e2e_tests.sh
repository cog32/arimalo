#!/usr/bin/env bash
set -euo pipefail

echo "Running E2E tests (desktop automation, one-shot)…"

exec node scripts/run_tauri_webdriver_e2e.mjs
