// Central app branding — the single frontend source for the product name.
//
// Override at build time (no code change) via Vite env vars:
//   VITE_APP_NAME    short brand  (sidebar, vault picker, PWA short_name)  default "Arimalo"
//   VITE_APP_TITLE   full title   (window / browser tab)                   default "Arimalo COVID"
//
// The native shell carries the same name in three static files that a runtime
// variable can't reach — keep these in sync when renaming:
//   - src-tauri/tauri.conf.json    (productName, window title)
//   - index.html                   (<title>, apple-mobile-web-app-title)
//   - public/manifest.webmanifest  (name, short_name)

const env = import.meta.env as Record<string, string | undefined>;

/** Short brand — shown in the sidebar and vault picker. */
export const APP_NAME = env.VITE_APP_NAME ?? "Arimalo";

/** Full window/tab title; defaults to "<APP_NAME> COVID" (the private variant). */
export const APP_TITLE = env.VITE_APP_TITLE ?? `${APP_NAME} COVID`;
