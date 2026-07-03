# Running the app on your phone (PWA)

The desktop app is a Tauri shell whose data calls go over Tauri IPC. In a phone
browser there is no IPC bridge, so `arimalo-web` serves the same frontend over
HTTP and exposes the **read-only** data commands at `POST /api/<command>`. Your
data and all computation stay on your Mac; the phone is a thin client.

It's read-only for now: reports, balances, ledger, and query work. Mutations
(rebuild from sources, CSV import, editing rules/trade links, plugins, "reveal
in Finder") are desktop-only and return an error in the browser. The on-screen
"Rebuild" becomes a refresh of the already-generated ledger.

> ⚠️ This serves your real financial data. Only expose it over a private network
> (Tailscale, or your home LAN) — never a public/port-forwarded address. On a
> non-loopback bind, a token is required (auto-generated if you don't pass one).

## Two ways to run it

### A. Single server (recommended for daily use on the phone)

Easiest — use the script. It builds the frontend + binary, generates a token,
and prints a phone-ready URL (using your Tailscale IP when available):

```bash
scripts/start_webserver.sh
# loopback only:        BIND=127.0.0.1:8787 scripts/start_webserver.sh
# fixed token:          TOKEN=mysecret scripts/start_webserver.sh
# fast restart:         SKIP_BUILD=1 scripts/start_webserver.sh
```

Or do it by hand:

```bash
npm run build
cargo build --manifest-path src-tauri/Cargo.toml --bin arimalo-web
./src-tauri/target/debug/arimalo-web --dist dist --bind 0.0.0.0:8787 --token "$(openssl rand -hex 16)"
```

It prints a URL with the token, e.g. `http://<host>:8787/?token=abc123…`. Open
that on your phone (over Tailscale use the Mac's tailnet IP/name). The token is
captured from the URL, stored for the session, and stripped from the address
bar; subsequent API calls send it as a bearer token. Static assets are served
unauthenticated (the shell is just code); the **data** is what the token gates.

Then use the browser's **Add to Home Screen** — the manifest + service worker
make it launch fullscreen like an app and load the shell offline (data calls
still need the Mac reachable).

Flags: `--bind` (default `127.0.0.1:8787`), `--dist` (default `dist`),
`--token`, `--app-data-dir` (override where the vault `config.json` is read
from). The active vault is whatever the desktop app last selected.

### B. Dev loop with live reload (for working on the UI from the phone)

Run the API server on loopback and Vite for the frontend with HMR:

```bash
# terminal 1 — data API on loopback (no token needed; only Vite talks to it)
./src-tauri/target/debug/arimalo-web --bind 127.0.0.1:8787

# terminal 2 — frontend dev server, reachable on the LAN
npm run dev
```

`vite.config.ts` sets `server.host` (so the phone can reach it) and proxies
`/api` → `127.0.0.1:8787`. Open `http://<mac-lan-ip>:1420/` on the phone; edits
to `src/` hot-reload on the device.

## How it works

- `src/ipc.ts` is the one import surface for `invoke` / `listen` / dialogs. In
  Tauri it delegates to the real Tauri APIs; in a browser `invoke()` becomes
  `fetch('/api/<command>')`, `listen()` is a no-op (no live pipeline events),
  and file/save dialogs are unavailable (`confirm` maps to the native dialog).
- `src-tauri/src/web/` resolves the vault from `config.json` (no Tauri
  `AppHandle`) and dispatches each command to the same library functions the
  desktop Tauri commands call, so the JSON is identical.

## Known limitations / follow-ups

- **Read-only.** Mutations and plugins are not wired (deliberately).
- **No live events.** If the desktop rebuilds while the phone is open, refresh
  to pick up changes.
- **Initial payload.** The startup load ships the full ledger (same as the
  desktop IPC payload); responses are not gzipped yet. Fine over Tailscale/LAN;
  gzip is an easy future win.
- **iOS standalone gotcha.** Add-to-Home-Screen runs in a fresh storage context;
  if the session token is lost, just re-open the `?token=` URL.
