use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;
use tiny_http::{Header, Request, Response, StatusCode};

use super::pairing::PairingStore;
use super::storage::GroupStorage;

#[derive(Serialize)]
struct PairInitiateResponse {
    group_id: String,
    pairing_code: String,
    expires_in: u64,
}

#[derive(Deserialize)]
struct PairJoinRequest {
    pairing_code: String,
}

#[derive(Serialize)]
struct PairJoinResponse {
    group_id: String,
}

#[derive(Serialize)]
struct OkResponse {
    ok: bool,
}

#[derive(Serialize)]
struct BlobListResponse {
    hashes: Vec<String>,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

fn json_response<T: Serialize>(status: u16, body: &T) -> Response<std::io::Cursor<Vec<u8>>> {
    let json = serde_json::to_vec(body).unwrap_or_default();
    let header = Header::from_bytes("Content-Type", "application/json").unwrap();
    Response::new(
        StatusCode(status),
        vec![header],
        std::io::Cursor::new(json.clone()),
        Some(json.len()),
        None,
    )
}

fn error_response(status: u16, msg: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    json_response(
        status,
        &ErrorResponse {
            error: msg.to_string(),
        },
    )
}

fn read_body(request: &mut Request, max_bytes: usize) -> Result<Vec<u8>, String> {
    let content_length = request.body_length().unwrap_or(0);
    if content_length > max_bytes {
        return Err("Body too large".to_string());
    }
    let mut body = Vec::with_capacity(content_length.min(max_bytes));
    request
        .as_reader()
        .take(max_bytes as u64)
        .read_to_end(&mut body)
        .map_err(|e| format!("Read body failed: {}", e))?;
    Ok(body)
}

// ── Pairing endpoints ──────────────────────────────────────────

pub fn handle_pair_initiate(request: Request, pairing: &PairingStore, ttl_secs: u64) {
    let result = pairing.initiate(ttl_secs);
    let resp = PairInitiateResponse {
        group_id: result.group_id,
        pairing_code: result.pairing_code,
        expires_in: result.expires_in,
    };
    let _ = request.respond(json_response(200, &resp));
}

pub fn handle_pair_join(mut request: Request, pairing: &PairingStore) {
    let body = match read_body(&mut request, 1024) {
        Ok(b) => b,
        Err(e) => {
            let _ = request.respond(error_response(400, &e));
            return;
        }
    };

    let join_req: PairJoinRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            let _ = request.respond(error_response(400, &format!("Invalid JSON: {}", e)));
            return;
        }
    };

    match pairing.join(&join_req.pairing_code) {
        Ok(group_id) => {
            let _ = request.respond(json_response(200, &PairJoinResponse { group_id }));
        }
        Err(_) => {
            let _ = request.respond(error_response(404, "Pairing code not found"));
        }
    }
}

// ── Metadata endpoints ─────────────────────────────────────────

pub fn handle_get_metadata(request: Request, data_dir: &Path, group_id: &str) {
    let storage = GroupStorage::new(data_dir, group_id);
    match storage.load_metadata() {
        Ok(Some(bytes)) => {
            let header = Header::from_bytes("Content-Type", "application/octet-stream").unwrap();
            let resp = Response::new(
                StatusCode(200),
                vec![header],
                std::io::Cursor::new(bytes.clone()),
                Some(bytes.len()),
                None,
            );
            let _ = request.respond(resp);
        }
        Ok(None) => {
            let _ = request.respond(error_response(404, "No metadata for this group"));
        }
        Err(e) => {
            let _ = request.respond(error_response(500, &e));
        }
    }
}

pub fn handle_post_metadata(mut request: Request, data_dir: &Path, group_id: &str) {
    let body = match read_body(&mut request, 50 * 1024 * 1024) {
        Ok(b) => b,
        Err(e) => {
            let _ = request.respond(error_response(400, &e));
            return;
        }
    };

    let storage = GroupStorage::new(data_dir, group_id);
    match storage.merge_and_save_metadata(&body) {
        Ok(()) => {
            let _ = request.respond(json_response(200, &OkResponse { ok: true }));
        }
        Err(e) => {
            let _ = request.respond(error_response(500, &e));
        }
    }
}

// ── Blob endpoints ─────────────────────────────────────────────

pub fn handle_get_blob(request: Request, data_dir: &Path, group_id: &str, hash: &str) {
    let storage = GroupStorage::new(data_dir, group_id);
    match storage.load_blob(hash) {
        Ok(Some(bytes)) => {
            let header = Header::from_bytes("Content-Type", "application/octet-stream").unwrap();
            let resp = Response::new(
                StatusCode(200),
                vec![header],
                std::io::Cursor::new(bytes.clone()),
                Some(bytes.len()),
                None,
            );
            let _ = request.respond(resp);
        }
        Ok(None) => {
            let _ = request.respond(error_response(404, "Blob not found"));
        }
        Err(e) => {
            let _ = request.respond(error_response(500, &e));
        }
    }
}

pub fn handle_post_blob(mut request: Request, data_dir: &Path, group_id: &str, hash: &str) {
    let body = match read_body(&mut request, 100 * 1024 * 1024) {
        Ok(b) => b,
        Err(e) => {
            let _ = request.respond(error_response(400, &e));
            return;
        }
    };

    let storage = GroupStorage::new(data_dir, group_id);
    match storage.store_blob(hash, &body) {
        Ok(()) => {
            let _ = request.respond(json_response(200, &OkResponse { ok: true }));
        }
        Err(e) => {
            let _ = request.respond(error_response(500, &e));
        }
    }
}

pub fn handle_list_blobs(request: Request, data_dir: &Path, group_id: &str) {
    let storage = GroupStorage::new(data_dir, group_id);
    match storage.list_blobs() {
        Ok(hashes) => {
            let _ = request.respond(json_response(200, &BlobListResponse { hashes }));
        }
        Err(e) => {
            let _ = request.respond(error_response(500, &e));
        }
    }
}

pub fn handle_not_found(request: Request) {
    let _ = request.respond(error_response(404, "Not found"));
}
