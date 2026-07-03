#![deny(warnings)]

//! arimalo-web — serve the frontend + read-only data API over HTTP so the app
//! can run in a phone browser (PWA). Data and compute stay on this machine; the
//! phone is a thin client. Reach it over Tailscale or your LAN.

use arimalo_covid::web::context::WebCtx;
use arimalo_covid::web::server::{WebServer, WebServerConfig};
use std::io::Read;
use std::path::PathBuf;

/// macOS app-data dir for the desktop app — holds the same config.json
/// (current_root / known_roots) the web server reads to find the active vault.
fn default_app_data_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join("Library/Application Support/com.cog32.arimalocovid")
}

fn is_loopback(bind: &str) -> bool {
    bind.starts_with("127.")
        || bind.starts_with("localhost")
        || bind.starts_with("[::1]")
        || bind.starts_with("::1")
}

/// 16 random bytes, hex-encoded. Falls back to a v4 UUID if /dev/urandom is
/// unavailable.
fn gen_token() -> String {
    let mut buf = [0u8; 16];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        if f.read_exact(&mut buf).is_ok() {
            return hex::encode(buf);
        }
    }
    uuid::Uuid::new_v4().simple().to_string()
}

fn print_help() {
    eprintln!("arimalo-web — serve the app as a PWA over HTTP");
    eprintln!();
    eprintln!("USAGE:");
    eprintln!("  arimalo-web [OPTIONS]");
    eprintln!();
    eprintln!("OPTIONS:");
    eprintln!("  --bind <ADDR>          Bind address (default: 127.0.0.1:8787)");
    eprintln!("  --dist <PATH>          Frontend dist directory (default: dist)");
    eprintln!("  --token <TOKEN>        Require this token on /api requests");
    eprintln!("  --app-data-dir <PATH>  Override the app-data dir (vault config)");
    eprintln!("  --help                 Show this help");
    eprintln!();
    eprintln!("Binding to a non-loopback address without --token auto-generates one.");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut bind = "127.0.0.1:8787".to_string();
    let mut dist = PathBuf::from("dist");
    let mut token: Option<String> = None;
    let mut app_data_dir = default_app_data_dir();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--bind" => {
                i += 1;
                if i < args.len() {
                    bind = args[i].clone();
                }
            }
            "--dist" => {
                i += 1;
                if i < args.len() {
                    dist = PathBuf::from(&args[i]);
                }
            }
            "--token" => {
                i += 1;
                if i < args.len() {
                    token = Some(args[i].clone());
                }
            }
            "--app-data-dir" => {
                i += 1;
                if i < args.len() {
                    app_data_dir = PathBuf::from(&args[i]);
                }
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            other => {
                eprintln!("Unknown argument: {other}");
                std::process::exit(1);
            }
        }
        i += 1;
    }

    // Safety: never expose a vault on a non-loopback address without a token.
    if token.is_none() && !is_loopback(&bind) {
        let generated = gen_token();
        eprintln!("⚠  Binding to non-loopback {bind} without --token — generated one.");
        token = Some(generated);
    }

    let ctx = WebCtx::from_app_data_dir(&app_data_dir);
    if ctx.config.current_root.is_none() && std::env::var("ARIMALO_GENERATED_DIR").is_err() {
        eprintln!(
            "⚠  No vault configured (no current_root in {}). Open the desktop app and pick a \
             vault first, or set ARIMALO_GENERATED_DIR / ARIMALO_SOURCES_DIR.",
            app_data_dir.display()
        );
    }

    let server = WebServer::new(
        WebServerConfig {
            bind: bind.clone(),
            dist_dir: dist.clone(),
            token: token.clone(),
        },
        ctx,
    )
    .unwrap_or_else(|e| {
        eprintln!("Failed to start: {e}");
        std::process::exit(1);
    });

    let addr = server.server_addr();
    eprintln!("Arimalo web server on http://{addr}");
    eprintln!("Serving frontend from: {}", dist.display());
    match &token {
        Some(t) => eprintln!("Open on your device: http://{addr}/?token={t}"),
        None => eprintln!("No auth token (loopback only)."),
    }
    server.run();
}
