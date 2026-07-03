#!/usr/bin/env bash
set -euo pipefail

git config core.hooksPath .githooks
chmod +x .githooks/pre-commit
chmod +x .githooks/pre-push || true
echo "Git hooks installed."
echo "- pre-commit: unit + BDD (optional E2E if runner installed)"
echo "- pre-push: unit + BDD (optional E2E when configured)"
