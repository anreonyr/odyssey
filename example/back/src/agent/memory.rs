//! Internal memory store for the agent. Previously the public
//! `memory` builtin exposing `memory_query`/`memory_insert` caps;
//! now it lives entirely inside the `agent` module, owned by
//! `AgentRuntime` as `Arc<dyn MemoryBackend>`. The agent's
//! `agent_memory_recall` and `agent_memory_record` caps call
//! methods on this backend directly; no cspace lookup, no
//! plugin-to-plugin contract.
//!
//! ## Backends
//!
//! Two implementations:
//! - `InMemoryBackend` — `RwLock<HashMap<String, Record>>`.
//!   Per-process, lost on restart. Used when
//!   `ODYSSEY_MEMORY_PATH` is unset.
//! - `FileMemoryBackend` — mirror of the in-memory store to a
//!   single JSON file. Survives restart. Selected by
//!   `ODYSSEY_MEMORY_PATH`.
//!
//! Both expose the same `MemoryBackend` trait. Adding new
//! backends (sqlite, sled, ...) is a thin newtype impl.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Record {
    pub id: String,
    pub content: Value,
    pub tags: Vec<String>,
    pub vector: Option<Vec<f32>>,
    pub created_at: String,
}

pub trait MemoryBackend: Send + Sync {
    fn insert(&self, rec: Record) -> String;
    fn query(
        &self,
        query: Option<&str>,
        vector: Option<&[f32]>,
        top_k: usize,
        filter_tags: &[String],
    ) -> Vec<Record>;
}

/// In-memory backend. MVP. Records live in a `HashMap`
/// protected by a `Mutex`. Tests reuse one instance per
/// `AgentRuntime`; production would swap this for a real store.
pub struct InMemoryBackend {
    store: Mutex<HashMap<String, Record>>,
    counter: Mutex<u64>,
}

impl InMemoryBackend {
    pub fn new() -> Self {
        Self {
            store: Mutex::new(HashMap::new()),
            counter: Mutex::new(0),
        }
    }

    fn next_id(&self) -> String {
        let mut n = self.counter.lock().expect("counter poisoned");
        *n += 1;
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        format!("mem_{ts}_{}", *n)
    }
}

impl Default for InMemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryBackend for InMemoryBackend {
    fn insert(&self, mut rec: Record) -> String {
        if rec.id.is_empty() {
            rec.id = self.next_id();
        }
        if rec.created_at.is_empty() {
            rec.created_at = now_iso8601();
        }
        let id = rec.id.clone();
        self.store
            .lock()
            .expect("store poisoned")
            .insert(id.clone(), rec);
        id
    }

    fn query(
        &self,
        query: Option<&str>,
        vector: Option<&[f32]>,
        top_k: usize,
        filter_tags: &[String],
    ) -> Vec<Record> {
        let store = self.store.lock().expect("store poisoned");
        let mut candidates: Vec<Record> = store
            .values()
            .filter(|r| filter_tags.is_empty() || filter_tags.iter().all(|t| r.tags.contains(t)))
            .cloned()
            .collect();

        if let Some(v) = vector {
            candidates.sort_by(|a, b| {
                let sa = a.vector.as_deref().map(|rv| cosine(rv, v)).unwrap_or(-1.0);
                let sb = b.vector.as_deref().map(|rv| cosine(rv, v)).unwrap_or(-1.0);
                sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
            });
        } else if let Some(q) = query {
            candidates.retain(|r| value_to_string(&r.content).contains(q));
            candidates.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        } else {
            candidates.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        }

        candidates.into_iter().take(top_k.max(1)).collect()
    }
}

/// File-backed `MemoryBackend` that mirrors the in-memory
/// store to a single JSON file. Records are loaded on
/// construction; the whole file is rewritten on every
/// `insert`. Simple and correct; future work would append +
/// compact or move to sled / sqlite.
pub struct FileMemoryBackend {
    path: std::path::PathBuf,
    store: Mutex<HashMap<String, Record>>,
}

impl FileMemoryBackend {
    /// Open (or create) the file at `path`, loading any
    /// existing records into memory.
    pub fn open(path: impl Into<std::path::PathBuf>) -> Result<Self, String> {
        let path = path.into();
        let store = if path.exists() {
            let bytes = std::fs::read(&path).map_err(|e| format!("memory file read: {e}"))?;
            if bytes.is_empty() {
                HashMap::new()
            } else {
                let v: serde_json::Value = serde_json::from_slice(&bytes)
                    .map_err(|e| format!("memory file parse: {e}"))?;
                let arr = v
                    .get("records")
                    .and_then(serde_json::Value::as_array)
                    .ok_or_else(|| "memory file: missing `records` array".to_string())?;
                let mut map = HashMap::new();
                for rec_value in arr {
                    let rec: Record = serde_json::from_value(rec_value.clone())
                        .map_err(|e| format!("memory file: record decode: {e}"))?;
                    map.insert(rec.id.clone(), rec);
                }
                map
            }
        } else {
            HashMap::new()
        };
        Ok(Self {
            path,
            store: Mutex::new(store),
        })
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    fn flush(&self, store: &HashMap<String, Record>) -> Result<(), String> {
        let mut records: Vec<&Record> = store.values().collect();
        records.sort_by(|a, b| a.id.cmp(&b.id));
        let arr: Vec<serde_json::Value> = records
            .iter()
            .map(|r| serde_json::to_value(r).expect("Record is serialisable"))
            .collect();
        let body = serde_json::json!({ "records": arr });
        let bytes =
            serde_json::to_vec_pretty(&body).map_err(|e| format!("memory file serialise: {e}"))?;
        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|e| format!("memory file mkdir: {e}"))?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, &bytes).map_err(|e| format!("memory file write: {e}"))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| format!("memory file rename: {e}"))?;
        Ok(())
    }
}

impl MemoryBackend for FileMemoryBackend {
    fn insert(&self, mut rec: Record) -> String {
        if rec.id.is_empty() {
            rec.id = next_file_id(&self.path);
        }
        if rec.created_at.is_empty() {
            rec.created_at = now_iso8601();
        }
        let id = rec.id.clone();
        {
            let mut store = self.store.lock().expect("store poisoned");
            store.insert(id.clone(), rec);
        }
        let store = self.store.lock().expect("store poisoned");
        if let Err(e) = self.flush(&store) {
            eprintln!("memory file: flush failed: {e}");
        }
        id
    }

    fn query(
        &self,
        query: Option<&str>,
        vector: Option<&[f32]>,
        top_k: usize,
        filter_tags: &[String],
    ) -> Vec<Record> {
        let snapshot: HashMap<String, Record> = self.store.lock().expect("store poisoned").clone();

        let mut candidates: Vec<Record> = snapshot
            .into_values()
            .filter(|r| filter_tags.is_empty() || filter_tags.iter().all(|t| r.tags.contains(t)))
            .collect();

        if let Some(v) = vector {
            candidates.sort_by(|a, b| {
                let sa = a.vector.as_deref().map(|rv| cosine(rv, v)).unwrap_or(-1.0);
                let sb = b.vector.as_deref().map(|rv| cosine(rv, v)).unwrap_or(-1.0);
                sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
            });
        } else if let Some(q) = query {
            candidates.retain(|r| r.content.to_string().contains(q));
            candidates.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        } else {
            candidates.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        }

        candidates.into_iter().take(top_k.max(1)).collect()
    }
}

/// Pick the right backend for the environment. `ODYSSEY_MEMORY_PATH`
/// selects `FileMemoryBackend`; otherwise an `InMemoryBackend`.
/// Returns `Arc<dyn MemoryBackend>`.
pub fn pick_backend() -> Arc<dyn MemoryBackend> {
    if let Ok(path) = std::env::var("ODYSSEY_MEMORY_PATH") {
        match FileMemoryBackend::open(&path) {
            Ok(b) => return Arc::new(b),
            Err(e) => panic!("memory: file backend open {path:?}: {e}"),
        }
    }
    Arc::new(InMemoryBackend::new())
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    if n == 0 {
        return 0.0;
    }
    let mut dot = 0.0;
    let mut na = 0.0;
    let mut nb = 0.0;
    for i in 0..n {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    let denom = (na.sqrt()) * (nb.sqrt());
    if denom == 0.0 { 0.0 } else { dot / denom }
}

fn value_to_string(v: &Value) -> String {
    v.to_string()
}

fn next_file_id(path: &std::path::Path) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
    format!("mem_{nanos}_{n}")
}

fn now_iso8601() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let secs = (now / 1000) as i64;
    let ms = (now % 1000) as u32;
    let (year, month, day, hour, min, sec) = epoch_to_ymdhms(secs);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}.{ms:03}Z")
}

fn epoch_to_ymdhms(secs: i64) -> (i32, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let s_of_day = secs.rem_euclid(86_400) as u32;
    let hour = s_of_day / 3600;
    let min = (s_of_day / 60) % 60;
    let sec = s_of_day % 60;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year as i32, m, d, hour, min, sec)
}

/// Public wrapper so the agent's Resources can call into the
/// store with the same shape the old `MemoryQueryResource` /
/// `MemoryInsertResource` accepted.
pub fn query_records(
    backend: &dyn MemoryBackend,
    query: Option<&str>,
    vector: Option<&[f32]>,
    top_k: usize,
    filter_tags: &[String],
) -> Result<Value, String> {
    if query.is_none() && vector.is_none() {
        return Err("memory_query: at least one of `query` or `vector` is required".to_string());
    }
    let hits = backend.query(query, vector, top_k, filter_tags);
    let hit_values: Vec<Value> = hits
        .into_iter()
        .map(|r| {
            json!({
                "id": r.id,
                "content": r.content,
                "tags": r.tags,
                "created_at": r.created_at,
                "score": 1.0,
            })
        })
        .collect();
    Ok(json!({ "hits": hit_values }))
}

pub fn insert_record(backend: &dyn MemoryBackend, input: Value) -> Result<Value, String> {
    if input.get("content").is_none() || input["content"].is_null() {
        return Err("memory_insert: `content` is required and must not be null".to_string());
    }
    let content = input["content"].clone();
    let tags: Vec<String> = input
        .get("tags")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(|s| s.to_string())
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();
    let vector: Option<Vec<f32>> = input.get("vector").and_then(Value::as_array).map(|arr| {
        arr.iter()
            .filter_map(Value::as_f64)
            .map(|n| n as f32)
            .collect::<Vec<f32>>()
    });
    let id_provided = input
        .get("id")
        .and_then(Value::as_str)
        .map(|s| s.to_string())
        .unwrap_or_default();
    let created_at = input
        .get("created_at")
        .and_then(Value::as_str)
        .map(|s| s.to_string())
        .unwrap_or_default();

    let rec = Record {
        id: id_provided,
        content,
        tags,
        vector,
        created_at,
    };

    let id = backend.insert(rec);
    Ok(json!({ "id": id }))
}
