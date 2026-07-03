#!/usr/bin/env bash
# Build and run the read-only PWA web server (arimalo-web) so the app can be
# used from a phone browser. Data + compute stay on this machine; the phone is
# a thin client. Reach it over Tailscale or your LAN. See docs/PWA.md.
#
# Usage:
#   scripts/start_webserver.sh                 # build, then serve on 0.0.0.0:8787 with a generated token
#   BIND=127.0.0.1:8787 scripts/start_webserver.sh   # loopback only (no token needed)
#   TOKEN=mysecret scripts/start_webserver.sh        # use a fixed token
#   SKIP_BUILD=1 scripts/start_webserver.sh          # skip npm/cargo build (fast restart)
#
# Any extra args are forwarded to the arimalo-web binary.
set -euo pipefail

cd "$(dirname "$0")/.."

BIND="${BIND:-0.0.0.0:8787}"
SKIP_BUILD="${SKIP_BUILD:-0}"

# A token is required unless binding to loopback. Generate one if not supplied.
TOKEN="${TOKEN:-}"
if [ -z "$TOKEN" ] && [[ "$BIND" != 127.* && "$BIND" != localhost* && "$BIND" != "[::1]"* ]]; then
  TOKEN="$(openssl rand -hex 16)"
fi

# Warn if no vault is configured (the desktop app sets this on first launch).
APP_DATA="$HOME/Library/Application Support/com.cog32.arimalocovid"
CONFIG_FILE="$APP_DATA/config.json"
ROOT=""
if [ -f "$CONFIG_FILE" ]; then
  ROOT=$(python3 -c "import json,sys; print(json.load(open(sys.argv[1])).get('current_root',''))" "$CONFIG_FILE" 2>/dev/null || echo "")
fi
if [ -z "$ROOT" ]; then
  echo "⚠  No vault configured in $CONFIG_FILE — open the desktop app and pick a vault first." >&2
else
  echo "Vault: $ROOT"
fi

if [ "$SKIP_BUILD" != "1" ]; then
  echo "Building frontend (npm run build)..."
  npm run build
  echo "Building arimalo-web..."
  cargo build --manifest-path src-tauri/Cargo.toml --bin arimalo-web
fi

BIN="src-tauri/target/debug/arimalo-web"
if [ ! -x "$BIN" ]; then
  echo "Error: $BIN not found. Run without SKIP_BUILD=1 to build it." >&2
  exit 1
fi

# Print a phone-friendly URL. On Tailscale, prefer the tailnet IP.
PORT="${BIND##*:}"
TS_IP="$(tailscale ip -4 2>/dev/null | head -1 || true)"
echo
echo "Serving on http://$BIND"
if [ -n "$TOKEN" ]; then
  HOST_FOR_URL="${TS_IP:-<this-machine>}"
  echo "Open on your phone:  http://$HOST_FOR_URL:$PORT/?token=$TOKEN"
else
  echo "Open locally:        http://127.0.0.1:$PORT/"
fi
echo

ARGS=(--dist dist --bind "$BIND")
if [ -n "$TOKEN" ]; then
  ARGS+=(--token "$TOKEN")
fi

exec "$BIN" "${ARGS[@]}" "$@"
