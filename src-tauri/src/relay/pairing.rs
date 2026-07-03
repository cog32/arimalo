use rand::Rng;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct PairingEntry {
    pub group_id: String,
    pub expires_at: u64,
    pub used: bool,
}

#[derive(Debug, Clone)]
pub struct PairingResult {
    pub group_id: String,
    pub pairing_code: String,
    pub expires_in: u64,
}

#[derive(Debug, Clone)]
pub struct PairingStore {
    entries: Arc<Mutex<HashMap<String, PairingEntry>>>,
}

impl Default for PairingStore {
    fn default() -> Self {
        Self::new()
    }
}

impl PairingStore {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Initiate pairing: create a new group and 6-digit code.
    /// `ttl_secs` controls how long the code is valid (default 600 = 10 minutes).
    pub fn initiate(&self, ttl_secs: u64) -> PairingResult {
        let group_id = Uuid::new_v4().to_string();
        let code = generate_code();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let entry = PairingEntry {
            group_id: group_id.clone(),
            expires_at: now + ttl_secs,
            used: false,
        };

        self.entries.lock().unwrap().insert(code.clone(), entry);

        PairingResult {
            group_id,
            pairing_code: code,
            expires_in: ttl_secs,
        }
    }

    /// Join with a pairing code. Returns the group_id if the code is valid,
    /// not expired, and not yet used. Marks the code as used on success.
    pub fn join(&self, code: &str) -> Result<String, String> {
        let mut entries = self.entries.lock().unwrap();
        let entry = entries
            .get_mut(code)
            .ok_or_else(|| "Pairing code not found".to_string())?;

        if entry.used {
            return Err("Pairing code already used".to_string());
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if now >= entry.expires_at {
            // Clean up expired code
            let code_owned = code.to_string();
            drop(entries);
            self.entries.lock().unwrap().remove(&code_owned);
            return Err("Pairing code not found".to_string());
        }

        entry.used = true;
        let group_id = entry.group_id.clone();

        Ok(group_id)
    }

    /// Clean up expired entries. Called periodically or on demand.
    pub fn cleanup_expired(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.entries
            .lock()
            .unwrap()
            .retain(|_, entry| entry.expires_at > now);
    }
}

fn generate_code() -> String {
    let mut rng = rand::thread_rng();
    let code: u32 = rng.gen_range(100_000..1_000_000);
    code.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initiate_returns_valid_result() {
        let store = PairingStore::new();
        let result = store.initiate(600);
        assert!(!result.group_id.is_empty());
        assert_eq!(result.pairing_code.len(), 6);
        assert_eq!(result.expires_in, 600);
    }

    #[test]
    fn test_join_with_valid_code() {
        let store = PairingStore::new();
        let result = store.initiate(600);
        let group_id = store.join(&result.pairing_code).unwrap();
        assert_eq!(group_id, result.group_id);
    }

    #[test]
    fn test_join_with_invalid_code() {
        let store = PairingStore::new();
        assert!(store.join("000000").is_err());
    }

    #[test]
    fn test_code_is_single_use() {
        let store = PairingStore::new();
        let result = store.initiate(600);
        store.join(&result.pairing_code).unwrap();
        assert!(store.join(&result.pairing_code).is_err());
    }

    #[test]
    fn test_expired_code_rejected() {
        let store = PairingStore::new();
        let result = store.initiate(0); // expires immediately
                                        // Small sleep to ensure time has passed
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(store.join(&result.pairing_code).is_err());
    }
}
