//! Standalone HTTP server that exposes the read-only data commands over
//! `POST /api/<command>` and serves the built frontend (`dist/`), so the app
//! can run in a phone browser as a PWA. The active vault is resolved from the
//! same `config.json` the desktop app writes — no Tauri `AppHandle` required.
//!
//! Read-only for now: mutations (rebuild/import/rules/trade-links) and live
//! pipeline events are deliberately not wired here yet.

pub mod context;
pub mod dispatch;
pub mod server;
