use std::path::{Path, PathBuf};
use std::sync::Arc;
use tiny_http::Server;

use super::handlers;
use super::pairing::PairingStore;

pub struct RelayConfig {
    pub bind: String,
    pub data_dir: PathBuf,
    pub pairing_ttl_secs: u64,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            bind: "0.0.0.0:8384".to_string(),
            data_dir: PathBuf::from("/tmp/arimalo-relay"),
            pairing_ttl_secs: 600,
        }
    }
}

pub struct RelayServer {
    server: Arc<Server>,
    pairing: PairingStore,
    data_dir: PathBuf,
    pairing_ttl_secs: u64,
}

impl std::fmt::Debug for RelayServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RelayServer")
            .field("data_dir", &self.data_dir)
            .field("pairing_ttl_secs", &self.pairing_ttl_secs)
            .finish()
    }
}

impl RelayServer {
    pub fn new(config: RelayConfig) -> Result<Self, String> {
        let server = Server::http(&config.bind)
            .map_err(|e| format!("Failed to bind {}: {}", config.bind, e))?;

        std::fs::create_dir_all(&config.data_dir)
            .map_err(|e| format!("Failed to create data dir: {}", e))?;

        Ok(Self {
            server: Arc::new(server),
            pairing: PairingStore::new(),
            data_dir: config.data_dir,
            pairing_ttl_secs: config.pairing_ttl_secs,
        })
    }

    /// Returns the server's actual bound address (useful for tests with port 0).
    pub fn server_addr(&self) -> String {
        self.server.server_addr().to_string()
    }

    pub fn pairing_store(&self) -> &PairingStore {
        &self.pairing
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Run the server (blocking). Call from the main thread or spawn in a thread.
    pub fn run(&self) {
        loop {
            let request = match self.server.recv() {
                Ok(r) => r,
                Err(_) => break,
            };
            self.handle_request(request);
        }
    }

    /// Handle a single request. Returns when done.
    pub fn handle_one(&self) -> bool {
        match self.server.recv() {
            Ok(request) => {
                self.handle_request(request);
                true
            }
            Err(_) => false,
        }
    }

    /// Unblock the server so `run` or `handle_one` will exit.
    pub fn unblock(&self) {
        self.server.unblock();
    }

    fn handle_request(&self, request: tiny_http::Request) {
        let method = request.method().to_string();
        let url = request.url().to_string();

        // Parse URL segments
        let segments: Vec<&str> = url
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        match (method.as_str(), segments.as_slice()) {
            // POST /pair/initiate
            ("POST", ["pair", "initiate"]) => {
                handlers::handle_pair_initiate(request, &self.pairing, self.pairing_ttl_secs);
            }

            // POST /pair/join
            ("POST", ["pair", "join"]) => {
                handlers::handle_pair_join(request, &self.pairing);
            }

            // GET /metadata/{group_id}
            ("GET", ["metadata", group_id]) => {
                handlers::handle_get_metadata(request, &self.data_dir, group_id);
            }

            // POST /metadata/{group_id}
            ("POST", ["metadata", group_id]) => {
                handlers::handle_post_metadata(request, &self.data_dir, group_id);
            }

            // GET /blobs/{group_id}/list
            ("GET", ["blobs", group_id, "list"]) => {
                handlers::handle_list_blobs(request, &self.data_dir, group_id);
            }

            // GET /blobs/{group_id}/{hash}
            ("GET", ["blobs", group_id, hash]) => {
                handlers::handle_get_blob(request, &self.data_dir, group_id, hash);
            }

            // POST /blobs/{group_id}/{hash}
            ("POST", ["blobs", group_id, hash]) => {
                handlers::handle_post_blob(request, &self.data_dir, group_id, hash);
            }

            _ => {
                handlers::handle_not_found(request);
            }
        }
    }
}
