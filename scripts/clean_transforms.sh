#!/usr/bin/env bash
# Remove auto-generated _transform.rhai files from the app's sources dir
# so the suggest flow regenerates them on next import.
set -euo pipefail

APP_ID="com.cog32.arimalocovid"

case "$(uname -s)" in
  Darwin) SOURCES_DIR="$HOME/Library/Application Support/$APP_ID/sources" ;;
  Linux)  SOURCES_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/$APP_ID/sources" ;;
  *)      SOURCES_DIR="$APPDATA/$APP_ID/sources" ;;
esac

if [ -d "$SOURCES_DIR" ]; then
  find "$SOURCES_DIR" -name '_transform.rhai' -delete 2>/dev/null || true
  find "$SOURCES_DIR" -name '_rules.json' -delete 2>/dev/null || true
  echo "Cleaned _transform.rhai and _rules.json files from $SOURCES_DIR"
fi
