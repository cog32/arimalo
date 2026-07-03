// IPC shim: one import surface for the data/dialog/event calls the app makes.
//
// In the Tauri desktop app these go over Tauri's IPC bridge. In a plain browser
// (the PWA served by `arimalo-web`) there is no bridge, so `invoke()` is routed
// to `POST /api/<command>` instead, and the dialog/event helpers fall back to
// browser equivalents.
//
// Swap `@tauri-apps/api/core` / `@tauri-apps/api/event` / `@tauri-apps/plugin-dialog`
// imports for this module; the call sites stay identical.

export type UnlistenFn = () => void;

/** True when running inside the Tauri webview (vs. a plain browser/PWA). */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

// ── auth token (PWA only) ───────────────────────────────────────────
// The user opens `http://host:8787/?token=XXX`; we capture the token, persist
// it for the session, and strip it from the URL bar. Subsequent /api calls send
// it as a bearer token.
const TOKEN_KEY = "sq_web_token";

function captureToken(): string | null {
  if (typeof window === "undefined") return null;
  try {
    const url = new URL(window.location.href);
    const fromUrl = url.searchParams.get("token");
    if (fromUrl) {
      sessionStorage.setItem(TOKEN_KEY, fromUrl);
      url.searchParams.delete("token");
      window.history.replaceState({}, "", url.toString());
      return fromUrl;
    }
    return sessionStorage.getItem(TOKEN_KEY);
  } catch {
    return null;
  }
}

const webToken = captureToken();

async function httpInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (webToken) headers.Authorization = `Bearer ${webToken}`;
  const res = await fetch(`/api/${cmd}`, {
    method: "POST",
    headers,
    body: JSON.stringify(args ?? {}),
  });
  if (!res.ok) {
    let msg = `HTTP ${res.status}`;
    try {
      const body = await res.json();
      if (body && typeof body.error === "string") msg = body.error;
    } catch {
      // non-JSON error body; keep the status message
    }
    throw new Error(msg);
  }
  return (await res.json()) as T;
}

/** Call a backend command. Tauri IPC on desktop, HTTP on the PWA. */
export async function invoke<T = unknown>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (isTauri()) {
    const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
    return tauriInvoke<T>(cmd, args);
  }
  return httpInvoke<T>(cmd, args);
}

/** Subscribe to a backend event. No-op in the browser (no live pipeline events). */
export async function listen<T = unknown>(
  event: string,
  handler: (event: { payload: T }) => void,
): Promise<UnlistenFn> {
  if (isTauri()) {
    const { listen: tauriListen } = await import("@tauri-apps/api/event");
    return tauriListen<T>(event, handler as (e: { payload: T }) => void);
  }
  return () => {};
}

// ── dialogs ─────────────────────────────────────────────────────────
// Browser file/save dialogs aren't available; the read-only PWA doesn't import
// or export files (those are desktop-only features for now). `confirm` maps to
// the native browser confirm. Each export is cast to the exact Tauri signature
// so the (overloaded) call sites type-check unchanged.

type OpenFn = (typeof import("@tauri-apps/plugin-dialog"))["open"];
type SaveFn = (typeof import("@tauri-apps/plugin-dialog"))["save"];
type ConfirmFn = (typeof import("@tauri-apps/plugin-dialog"))["confirm"];

export const open = (async (options?: unknown) => {
  if (isTauri()) {
    const mod = await import("@tauri-apps/plugin-dialog");
    return mod.open(options as never);
  }
  throw new Error("File selection is not available in the browser (desktop-only feature)");
}) as OpenFn;

export const save = (async (options?: unknown) => {
  if (isTauri()) {
    const mod = await import("@tauri-apps/plugin-dialog");
    return mod.save(options as never);
  }
  throw new Error("Saving files is not available in the browser (desktop-only feature)");
}) as SaveFn;

export const confirm = (async (message: string, options?: unknown) => {
  if (isTauri()) {
    const mod = await import("@tauri-apps/plugin-dialog");
    return mod.confirm(message, options as never);
  }
  if (typeof window !== "undefined") return window.confirm(message);
  return false;
}) as ConfirmFn;
