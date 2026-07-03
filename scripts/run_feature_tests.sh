#!/usr/bin/env bash
set -euo pipefail

echo "Running parser BDD feature tests (Rust, one-shot)…"
npm run -s test:bdd:parser

echo "Verifying UI cucumber suite is fully defined (dry-run)…"
# The UI suite (features/ui) is executed for real via `npm run e2e` against
# a built Tauri app. Here we only verify in CI that every non-@wip scenario
# has a matching step definition — so the suite cannot silently grow new
# undefined scenarios. Scenarios that still need step impls must be tagged
# @wip until they are wired up; @appium scenarios are mac-only.
out=$(npx cucumber-js --dry-run --tags "not @wip and not @appium" 2>&1)
echo "$out"
if echo "$out" | grep -Eq '\b[1-9][0-9]* undefined\b'; then
  echo "ERROR: UI cucumber suite has undefined steps in non-@wip scenarios." >&2
  echo "       Implement the missing step files or tag the scenario @wip." >&2
  exit 1
fi
