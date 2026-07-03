import { defineConfig } from "vite";

export default defineConfig({
  server: {
    port: 1420,
    strictPort: true,
    // Listen on all interfaces so a phone on the same LAN/Tailnet can load the
    // dev server with live reload. Data calls are proxied to arimalo-web.
    host: true,
    proxy: {
      "/api": "http://127.0.0.1:8787",
    },
  },
  clearScreen: false,
});
