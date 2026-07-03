use std::path::{Path, PathBuf};
use std::sync::Arc;

use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

use super::context::WebCtx;
use super::dispatch;

pub struct WebServerConfig {
    pub bind: String,
    pub dist_dir: PathBuf,
    /// When `Some`, every `/api/*` request must present this token via
    /// `Authorization: Bearer <token>` or `?token=<token>`. Static assets are
    /// served unauthenticated (the shell is just code; the data is gated).
    pub token: Option<String>,
}

pub struct WebServer {
    server: Arc<Server>,
    ctx: Arc<WebCtx>,
    dist_dir: PathBuf,
    token: Option<String>,
}

impl WebServer {
    pub fn new(config: WebServerConfig, ctx: WebCtx) -> Result<Self, String> {
        let server = Server::http(&config.bind)
            .map_err(|e| format!("Failed to bind {}: {}", config.bind, e))?;
        Ok(Self {
            server: Arc::new(server),
            ctx: Arc::new(ctx),
            dist_dir: config.dist_dir,
            token: config.token,
        })
    }

    /// The actual bound address (useful when binding to port 0 in tests).
    pub fn server_addr(&self) -> String {
        self.server.server_addr().to_string()
    }

    /// Run the server loop (blocking).
    pub fn run(&self) {
        loop {
            match self.server.recv() {
                Ok(request) => self.handle(request),
                Err(_) => break,
            }
        }
    }

    /// Handle a single request (used by tests).
    pub fn handle_one(&self) -> bool {
        match self.server.recv() {
            Ok(request) => {
                self.handle(request);
                true
            }
            Err(_) => false,
        }
    }

    /// Unblock `run` / `handle_one`.
    pub fn unblock(&self) {
        self.server.unblock();
    }

    fn handle(&self, request: Request) {
        let method = request.method().clone();
        let raw_url = request.url().to_string();
        let (path, query) = split_query(&raw_url);

        if let Some(cmd) = path.strip_prefix("/api/") {
            if method != Method::Post {
                let _ = request.respond(error_response(405, "method not allowed"));
                return;
            }
            if !self.authorized(&request, query) {
                let _ = request.respond(error_response(401, "unauthorized"));
                return;
            }
            self.handle_api(request, cmd.to_string());
            return;
        }

        if method != Method::Get && method != Method::Head {
            let _ = request.respond(error_response(405, "method not allowed"));
            return;
        }
        self.serve_static(request, path);
    }

    fn authorized(&self, request: &Request, query: &str) -> bool {
        match &self.token {
            None => true,
            Some(expected) => extract_token(request, query).as_deref() == Some(expected.as_str()),
        }
    }

    fn handle_api(&self, mut request: Request, cmd: String) {
        let mut body = Vec::new();
        if request.as_reader().read_to_end(&mut body).is_err() {
            let _ = request.respond(error_response(400, "failed to read body"));
            return;
        }
        let args: serde_json::Value = if body.is_empty() {
            serde_json::Value::Object(Default::default())
        } else {
            match serde_json::from_slice(&body) {
                Ok(v) => v,
                Err(e) => {
                    let _ = request.respond(error_response(400, &format!("invalid JSON: {e}")));
                    return;
                }
            }
        };
        match dispatch::dispatch(&self.ctx, &cmd, &args) {
            Ok(value) => {
                let bytes = serde_json::to_vec(&value).unwrap_or_default();
                let _ = request.respond(bytes_response(200, "application/json", bytes));
            }
            // 422: dispatch ran but the command rejected — surfaces to the
            // frontend exactly like an invoke() rejection.
            Err(e) => {
                let _ = request.respond(error_response(422, &e));
            }
        }
    }

    fn serve_static(&self, request: Request, path: &str) {
        let rel = path.trim_start_matches('/');
        let rel = if rel.is_empty() { "index.html" } else { rel };
        // Block path traversal.
        if rel.split('/').any(|seg| seg == ".." || seg == ".") {
            let _ = request.respond(error_response(400, "bad path"));
            return;
        }
        let mut file_path = self.dist_dir.join(rel);
        if !file_path.is_file() {
            // Fall back to index.html so deep links / refreshes still load.
            file_path = self.dist_dir.join("index.html");
        }
        match std::fs::read(&file_path) {
            Ok(bytes) => {
                let _ = request.respond(bytes_response(200, content_type(&file_path), bytes));
            }
            Err(_) => {
                let _ = request.respond(error_response(404, "not found"));
            }
        }
    }
}

fn bytes_response(
    status: u16,
    content_type: &str,
    bytes: Vec<u8>,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let header = Header::from_bytes("Content-Type", content_type)
        .unwrap_or_else(|_| Header::from_bytes("Content-Type", "application/octet-stream").unwrap());
    let len = bytes.len();
    Response::new(
        StatusCode(status),
        vec![header],
        std::io::Cursor::new(bytes),
        Some(len),
        None,
    )
}

fn error_response(status: u16, msg: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = serde_json::to_vec(&serde_json::json!({ "error": msg })).unwrap_or_default();
    bytes_response(status, "application/json", body)
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("webmanifest") => "application/manifest+json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        Some("map") => "application/json",
        _ => "application/octet-stream",
    }
}

fn split_query(url: &str) -> (&str, &str) {
    match url.split_once('?') {
        Some((p, q)) => (p, q),
        None => (url, ""),
    }
}

fn extract_token(request: &Request, query: &str) -> Option<String> {
    for h in request.headers() {
        if h.field.equiv("Authorization") {
            if let Some(t) = h.value.as_str().strip_prefix("Bearer ") {
                return Some(t.to_string());
            }
        }
    }
    for pair in query.split('&') {
        if let Some(t) = pair.strip_prefix("token=") {
            return Some(t.to_string());
        }
    }
    None
}
