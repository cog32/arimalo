// Service worker for the PWA build.
//
// - App shell (HTML/JS/CSS/icons) is cached network-first so the UI loads
//   offline after a first online visit.
// - /api/* requests are NEVER cached — financial data must be fresh; offline
//   API calls simply fail and the app surfaces the error.

const CACHE = "arimalo-shell-v1";
const SHELL = ["/", "/index.html", "/manifest.webmanifest"];

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches
      .open(CACHE)
      .then((cache) => cache.addAll(SHELL))
      .then(() => self.skipWaiting()),
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) =>
        Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k))),
      )
      .then(() => self.clients.claim()),
  );
});

self.addEventListener("fetch", (event) => {
  const url = new URL(event.request.url);

  // Data API: bypass the cache entirely.
  if (url.pathname.startsWith("/api/")) return;
  if (event.request.method !== "GET") return;

  // Network-first for the shell; fall back to cache (then index.html) offline.
  event.respondWith(
    fetch(event.request)
      .then((response) => {
        const copy = response.clone();
        caches
          .open(CACHE)
          .then((cache) => cache.put(event.request, copy))
          .catch(() => {});
        return response;
      })
      .catch(() =>
        caches
          .match(event.request)
          .then((cached) => cached || caches.match("/index.html")),
      ),
  );
});
