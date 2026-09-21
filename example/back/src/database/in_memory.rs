//! In-memory `Database` impl. Records live in a
//! `RwLock<HashMap<String, Value>>`. Per-process, lost on
//! restart — file-backed persistence is a separate impl
//! (`database/file.rs`, future work).

use std::collections::HashMap;
use std::sync::RwLock;

use serde_json::Value;

use super::Database;

pub struct InMemoryDatabase {
    store: RwLock<HashMap<String, Value>>,
}

impl InMemoryDatabase {
    pub fn new() -> Self {
        Self {
            store: RwLock::new(HashMap::new()),
        }
    }
}

impl Database for InMemoryDatabase {
    fn get(&self, key: &str) -> Result<Option<Value>, String> {
        let store = self
            .store
            .read()
            .map_err(|e| format!("database: lock poisoned: {e}"))?;
        Ok(store.get(key).cloned())
    }

    fn set(&self, key: &str, value: Value) -> Result<(), String> {
        let mut store = self
            .store
            .write()
            .map_err(|e| format!("database: lock poisoned: {e}"))?;
        store.insert(key.to_string(), value);
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<bool, String> {
        let mut store = self
            .store
            .write()
            .map_err(|e| format!("database: lock poisoned: {e}"))?;
        Ok(store.remove(key).is_some())
    }
}

impl Default for InMemoryDatabase {
    fn default() -> Self {
        Self::new()
    }
}
