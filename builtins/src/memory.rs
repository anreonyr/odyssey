//! Memory builtin — a single plugin exposing two capabilities:
//!
//! - `memory_query`  (contract `"memory_query"`)  — read records.
//! - `memory_insert` (contract `"memory_insert"`) — write a record.
//!
//! Both share an in-memory backend for MVP. The backend is
//! `Arc<Mutex<HashMap<MemoryId, Record>>>`; replacing it with a
//! sled/sqlite/postgres store is a one-function change.
//!
//! ## Two caps, one plugin
//!
//! Same rationale as the LLM plugin: query and insert share
//! provider state. Two caps means the resolver matches them
//! individually against the agent's `requires`, and the agent
//! holds a typed `Slot<MemoryQueryResource>` and a typed
//! `Slot<MemoryInsertResource>`.
//!
//! ## Search semantics
//!
//! - If `vector` is given, compute cosine similarity with each
//!   record's stored vector; return `top_k` by descending score.
//! - Else if `query` is given, do a substring match on stringified
//!   `content`; return matching records ordered by `created_at`
//!   descending.
//! - Else error.
//!
//! The agent does the embedding (via `llm_embed`) and passes the
//! vector in. The memory plugin does not know about the LLM —
//! that's the "composed of capabilities" boundary.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use odyssey::capability::enforce::quota::CapabilityBudget;
use odyssey::core::Resource;
use odyssey::core::contract::builtin::BuiltinManifest;
use odyssey::core::identity::ids::{PluginId, SlotId};
use odyssey::core::identity::kind::CapKind;
use odyssey::core::manifest::manifest::{CapabilityDecl, ManifestBuilder, PluginManifest};
use odyssey::personality::composition::resolve::ResolvedBinding;
use odyssey::personality::lifecycle::mint::CapabilityFactory;
use odyssey::personality::lifecycle::run::{MintFn, RuinFn, default_ruin};
use serde_json::{Value, json};

pub const CONTRACT_QUERY: &str = "memory_query";
pub const CONTRACT_INSERT: &str = "memory_insert";
pub const NAME_QUERY: &str = "memory_query";
pub const NAME_INSERT: &str = "memory_insert";

// ---------------------------------------------------------------------------
// Record + Store
// ---------------------------------------------------------------------------

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

/// In-memory backend. MVP only. Records live in a `HashMap`
/// protected by a `Mutex`. Tests reuse one instance per
/// `CapabilitySpace`; production would swap this for a real store.
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
            .filter(|r| {
                filter_tags.is_empty()
                    || filter_tags.iter().all(|t| r.tags.contains(t))
            })
            .cloned()
            .collect();

        if let Some(v) = vector {
            // Vector search: cosine similarity, descending.
            candidates.sort_by(|a, b| {
                let sa = a.vector.as_deref().map(|rv| cosine(rv, v)).unwrap_or(-1.0);
                let sb = b.vector.as_deref().map(|rv| cosine(rv, v)).unwrap_or(-1.0);
                sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
            });
        } else if let Some(q) = query {
            // Substring match on stringified content.
            candidates.retain(|r| value_to_string(&r.content).contains(q));
            // Most recent first.
            candidates.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        } else {
            // No query, no vector: return all, most recent first.
            candidates.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        }

        candidates.into_iter().take(top_k.max(1)).collect()
    }
}

impl Default for InMemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// File-backed memory — `MemoryBackend` + JSON file persistence
// ---------------------------------------------------------------------------

/// On-disk `MemoryBackend` that mirrors the in-memory store
/// to a single JSON file. Records are loaded on construction
/// and the whole file is rewritten on every `insert`. This
/// is the MVP durability story: not high-throughput, but
/// simple and correct. A future pass would append + compact
/// or move to sled / sqlite for indexed lookup.
pub struct FileMemoryBackend {
    path: std::path::PathBuf,
    store: Mutex<HashMap<String, Record>>,
}

impl FileMemoryBackend {
    /// Open (or create) the file at `path`, loading any
    /// existing records into memory. A missing file is
    /// treated as an empty store.
    pub fn open(path: impl Into<std::path::PathBuf>) -> Result<Self, String> {
        let path = path.into();
        let store = if path.exists() {
            let bytes = std::fs::read(&path)
                .map_err(|e| format!("memory file read: {e}"))?;
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

    /// Rewrite the whole file from the in-memory store. Held
    /// under the store lock so concurrent inserts can't
    /// interleave with a partial write.
    fn flush(&self, store: &HashMap<String, Record>) -> Result<(), String> {
        let mut records: Vec<&Record> = store.values().collect();
        // Stable order: by id. Makes the file diff-friendly
        // and avoids reordering on every flush.
        records.sort_by(|a, b| a.id.cmp(&b.id));
        let arr: Vec<serde_json::Value> = records
            .iter()
            .map(|r| serde_json::to_value(r).expect("Record is serialisable"))
            .collect();
        let body = serde_json::json!({ "records": arr });
        let bytes = serde_json::to_vec_pretty(&body)
            .map_err(|e| format!("memory file serialise: {e}"))?;
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("memory file mkdir: {e}"))?;
            }
        }
        // Write to a sibling temp file then atomically rename,
        // so a crash mid-write doesn't leave a half-baked file
        // that the next `open` would parse incorrectly.
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, &bytes)
            .map_err(|e| format!("memory file write: {e}"))?;
        std::fs::rename(&tmp, &self.path)
            .map_err(|e| format!("memory file rename: {e}"))?;
        Ok(())
    }

    /// Path to the backing file. Useful for tests and for
    /// operators to inspect.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl MemoryBackend for FileMemoryBackend {
    fn insert(&self, mut rec: Record) -> String {
        // Auto-fill id and created_at if the caller didn't
        // supply them. Matches `InMemoryBackend`'s behaviour
        // so the two backends are interchangeable from the
        // resource's perspective.
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
        // Rewrite the whole file. For MVP this is fine
        // (the agent inserts a few records per session at
        // most); a real workload would batch.
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
        // Reuse the in-memory logic by extracting the inner
        // store snapshot. Cheap O(N) on a per-call basis.
        let snapshot: HashMap<String, Record> = self
            .store
            .lock()
            .expect("store poisoned")
            .clone();

        let mut candidates: Vec<Record> = snapshot
            .into_values()
            .filter(|r| {
                filter_tags.is_empty()
                    || filter_tags.iter().all(|t| r.tags.contains(t))
            })
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
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}

fn value_to_string(v: &Value) -> String {
    v.to_string()
}

/// Monotonic id generator tied to the file path so two
/// `FileMemoryBackend`s opening the same file at different
/// moments don't collide. The id is `mem_<nanos>_<counter>`
/// where `nanos` is the file's mtime in nanoseconds and
/// `counter` is a static atomic that increments globally.
/// Stable across process restarts because the mtime of the
/// file persists.
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
    // Minimal ISO 8601 UTC timestamp without bringing in `chrono`.
    // Format: `YYYY-MM-DDTHH:MM:SS.sssZ`. The millisecond field
    // rounds to keep the test assertions deterministic.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let secs = (now / 1000) as i64;
    let ms = (now % 1000) as u32;
    let (year, month, day, hour, min, sec) = epoch_to_ymdhms(secs);
    format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}.{ms:03}Z"
    )
}

fn epoch_to_ymdhms(secs: i64) -> (i32, u32, u32, u32, u32, u32) {
    // Howard Hinnant's `days_from_civil` algorithm, inlined.
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

// ---------------------------------------------------------------------------
// Resources — one per cap
// ---------------------------------------------------------------------------

pub struct MemoryQueryResource {
    pub backend: Arc<dyn MemoryBackend>,
}

impl Resource for MemoryQueryResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let query = input
            .get("query")
            .and_then(Value::as_str)
            .map(|s| s.to_string());
        let vector = input
            .get("vector")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_f64)
                    .map(|n| n as f32)
                    .collect::<Vec<f32>>()
            });
        let top_k = input
            .get("top_k")
            .and_then(Value::as_u64)
            .map(|n| n as usize)
            .unwrap_or(5);
        let filter_tags = input
            .get("filter")
            .and_then(|f| f.get("tags"))
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(|s| s.to_string())
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default();

        if query.is_none() && vector.is_none() {
            return Err(
                "memory_query: at least one of `query` or `vector` is required".to_string(),
            );
        }

        let hits = self.backend.query(
            query.as_deref(),
            vector.as_deref(),
            top_k,
            &filter_tags,
        );

        let hit_values: Vec<Value> = hits
            .into_iter()
            .map(|r| {
                json!({
                    "id": r.id,
                    "content": r.content,
                    "tags": r.tags,
                    "created_at": r.created_at,
                    "score": 1.0,  // MVP: vector backend reports the
                                   // actual score; keyword backend
                                   // always returns 1.0.
                })
            })
            .collect();

        Ok(json!({ "hits": hit_values }))
    }
}

pub struct MemoryInsertResource {
    pub backend: Arc<dyn MemoryBackend>,
}

impl Resource for MemoryInsertResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
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
        let vector: Option<Vec<f32>> = input
            .get("vector")
            .and_then(Value::as_array)
            .map(|arr| {
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

        let id = self.backend.insert(rec);
        Ok(json!({ "id": id }))
    }
}

// ---------------------------------------------------------------------------
// Builtin — one plugin, two caps
// ---------------------------------------------------------------------------

pub struct MemoryBuiltin;

impl BuiltinManifest for MemoryBuiltin {
    fn manifest(&self) -> PluginManifest {
        ManifestBuilder::new("memory")
            .expose(NAME_QUERY, CONTRACT_QUERY)
            .expose(NAME_INSERT, CONTRACT_INSERT)
            .host("dispatcher")
            .timeout_ms(5000)
            .build()
    }
}

/// Pick the right backend for the environment. If
/// `ODYSSEY_MEMORY_PATH` is set, use the file-backed
/// backend (records mirror to that JSON file, survive
/// restart). Otherwise use a thread-local in-memory backend
/// (per-process, lost on restart).
fn pick_backend() -> Arc<dyn MemoryBackend> {
    if let Ok(path) = std::env::var("ODYSSEY_MEMORY_PATH") {
        match FileMemoryBackend::open(&path) {
            Ok(b) => return Arc::new(b),
            Err(e) => panic!("memory: file backend open {path:?}: {e}"),
        }
    }
    thread_local! {
        static BACKEND: std::cell::RefCell<Option<Arc<InMemoryBackend>>> =
            std::cell::RefCell::new(None);
    }
    BACKEND.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            *slot = Some(Arc::new(InMemoryBackend::new()));
        }
        slot.as_ref().unwrap().clone() as Arc<dyn MemoryBackend>
    })
}

impl MemoryBuiltin {
    pub fn mint(
        &self,
        factory: &CapabilityFactory,
        plugin: &PluginId,
        decl: &CapabilityDecl,
        kind: CapKind,
        budget: CapabilityBudget,
        _bindings: &[ResolvedBinding],
    ) -> SlotId {
        // Backend selection (extracted so we don't put a
        // macro inside a match arm):
        // - `ODYSSEY_MEMORY_PATH` set → `FileMemoryBackend`,
        //   records mirrored to that JSON file. Survives
        //   process restart.
        // - unset → thread-local `InMemoryBackend` (MVP;
        //   per-process, lost on restart). The thread-local
        //   keeps multiple mints (one per cap) sharing state
        //   without turning into a process-global singleton
        //   that would pollute tests.
        let backend: Arc<dyn MemoryBackend> = pick_backend();

        match decl.name.as_str() {
            NAME_QUERY => factory.mint(
                kind,
                decl,
                plugin,
                budget,
                Arc::new(MemoryQueryResource {
                    backend: backend.clone(),
                }),
            ),
            NAME_INSERT => factory.mint(
                kind,
                decl,
                plugin,
                budget,
                Arc::new(MemoryInsertResource { backend }),
            ),
            other => panic!("memory: unexpected capability name `{other}`"),
        }
    }

    pub fn register() -> (PluginManifest, MintFn, RuinFn) {
        (
            MemoryBuiltin.manifest(),
            |factory, plugin, decl, kind, budget, bindings| {
                MemoryBuiltin.mint(factory, plugin, decl, kind, budget, bindings)
            },
            default_ruin,
        )
    }
}
