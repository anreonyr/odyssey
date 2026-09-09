//! Capability model — seL4-inspired, typed by resource and kind.
//!
//! ## Two axes of typing
//!
//! 1. **Resource** (`R`) — what the plugin module declared. `R: SyncResource`
//!    for sync capabilities, `R: StreamResource` for streaming.
//!
//! 2. **Kind** (`K`) — `SyncKind` or `StreamKind`. The kind is encoded as a
//!    `PhantomData` parameter so the type system distinguishes sync and
//!    streaming capabilities, and `AnyCapability` can have non-overlapping
//!    impls for each.
//!
//! Together: `Capability<EchoResource, SyncKind>` vs
//! `Capability<StreamEchoResource, StreamKind>`. Each is its own concrete
//! type — the compiler will not let you accidentally treat one as the
//! other, and `AnyCapability` is implemented once per kind so no
//! coherence conflict arises.
//!
//! ## seL4 mapping
//!
//!   CNode slot           → Capability<R, K>
//!   Kernel object        → R (the resource, allocated by the plugin)
//!   seL4_Send            → Capability::invoke (K=SyncKind) / open (K=StreamKind)
//!   Resource badge       → CapabilityBudget.timeout_ms (per-call wall clock)
//!
//! ## Type erasure
//!
//! The shared `CapabilityService` registry holds `Arc<dyn AnyCapability>`
//! so capabilities of different resource types can coexist. The HTTP
//! bridge and pipeline use this erased interface.

use std::marker::PhantomData;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::Value;
use tokio::sync::mpsc;

use crate::manifest::{CapabilityDecl, PluginId};

// ---------------------------------------------------------------------------
// Kinds — type-level distinction between sync and streaming capabilities
// ---------------------------------------------------------------------------

/// Marker for sync capabilities.
pub struct SyncKind;
/// Marker for streaming capabilities.
pub struct StreamKind;

/// Capability-kind metadata: streaming bit at the type level.
pub trait CapabilityKind: Send + Sync + 'static {
    #[allow(dead_code)]
    const STREAMING: bool;
}
impl CapabilityKind for SyncKind {
    const STREAMING: bool = false;
}
impl CapabilityKind for StreamKind {
    const STREAMING: bool = true;
}

// ---------------------------------------------------------------------------
// Basic types
// ---------------------------------------------------------------------------

/// Unforgeable capability identifier.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CapabilityId(pub u64);

impl std::fmt::Display for CapabilityId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cap:{}", self.0)
    }
}

#[derive(Clone, Debug)]
pub struct CapabilityMeta {
    pub id: CapabilityId,
    pub name: String,
    pub plugin: PluginId,
    pub in_type: String,
    pub out_type: String,
    pub streaming: bool,
    pub timeout_ms: u32,
}

#[derive(Clone, Debug)]
pub struct CapabilityBudget {
    pub timeout_ms: u32,
}

impl CapabilityBudget {
    pub fn new(timeout_ms: u32) -> Self {
        Self { timeout_ms }
    }
}

#[derive(Debug)]
pub enum CapabilityChunk<T = Value> {
    Item(T),
    Done,
}

/// SyncKind resource: the body of a sync capability.
pub trait SyncResource: Send + Sync + 'static {
    fn invoke(&self, input: Value) -> Result<Value, String>;
}

/// Streaming resource: the body of a streaming capability.
pub trait StreamResource: Send + Sync + 'static {
    fn open(&self, input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String>;
}

// ---------------------------------------------------------------------------
// Capability<R, K> — typed capability handle
// ---------------------------------------------------------------------------

/// Unforgeable capability handle, generic over the resource type `R` and
/// the kind `K` (`SyncKind` or `StreamKind`). The kind is encoded as
/// `PhantomData` so it carries no runtime cost.
pub struct Capability<R: Send + Sync + 'static, K: CapabilityKind = SyncKind> {
    meta: CapabilityMeta,
    handler: Arc<R>,
    budget: Arc<CapabilityBudget>,
    _kind: PhantomData<K>,
}

impl<R: Send + Sync + 'static, K: CapabilityKind> Capability<R, K> {
    pub fn meta(&self) -> &CapabilityMeta {
        &self.meta
    }

    pub fn name(&self) -> &str {
        &self.meta.name
    }

    pub fn id(&self) -> CapabilityId {
        self.meta.id.clone()
    }

    /// Internal constructor used by the factory. Crate-internal.
    pub(crate) fn new_typed(
        meta: CapabilityMeta,
        handler: Arc<R>,
        budget: Arc<CapabilityBudget>,
    ) -> Self {
        Self { meta, handler, budget, _kind: PhantomData }
    }
}

impl<R: Send + Sync + 'static, K: CapabilityKind> Clone for Capability<R, K> {
    fn clone(&self) -> Self {
        Self {
            meta: self.meta.clone(),
            handler: self.handler.clone(),
            budget: self.budget.clone(),
            _kind: PhantomData,
        }
    }
}

impl<R: SyncResource + 'static> Capability<R, SyncKind> {
    /// Invoke the resource. If elapsed wall-clock time exceeds the
    /// token's `timeout_ms`, returns `Err` and the handler result is dropped.
    pub fn invoke(&self, input: Value) -> Result<Value, String> {
        let start = Instant::now();
        let result = self.handler.invoke(input);
        let elapsed_ms = start.elapsed().as_millis() as u64;
        if elapsed_ms > self.budget.timeout_ms as u64 {
            return Err(format!(
                "{}: timeout {}ms exceeded budget {}ms",
                self.meta.name, elapsed_ms, self.budget.timeout_ms
            ));
        }
        result
    }
}

impl<R: StreamResource + 'static> Capability<R, StreamKind> {
    /// Open the stream. Per-chunk delivery is the resource's job; the
    /// budget governs the open-to-last-chunk window for the caller.
    pub fn open(&self, input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String> {
        self.handler.open(input)
    }
}

// ---------------------------------------------------------------------------
// Type-erased view — for CapabilityService, HTTP bridge, pipeline
// ---------------------------------------------------------------------------

/// Erased capability: lets heterogeneous `Capability<R, K>` values
/// coexist in a single registry.
pub trait AnyCapability: Send + Sync {
    fn meta(&self) -> &CapabilityMeta;
    fn is_streaming(&self) -> bool;
    fn invoke_dyn(&self, input: Value) -> Result<Value, String>;
    fn open_dyn(&self, input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String>;
}

impl<R: SyncResource + 'static> AnyCapability for Capability<R, SyncKind> {
    fn meta(&self) -> &CapabilityMeta {
        &self.meta
    }
    fn is_streaming(&self) -> bool {
        false
    }
    fn invoke_dyn(&self, input: Value) -> Result<Value, String> {
        self.invoke(input)
    }
    fn open_dyn(&self, _: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String> {
        Err(format!("{} is sync; use invoke", self.meta.name))
    }
}

impl<R: StreamResource + 'static> AnyCapability for Capability<R, StreamKind> {
    fn meta(&self) -> &CapabilityMeta {
        &self.meta
    }
    fn is_streaming(&self) -> bool {
        true
    }
    fn invoke_dyn(&self, _: Value) -> Result<Value, String> {
        Err(format!("{} is streaming; use open", self.meta.name))
    }
    fn open_dyn(&self, input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String> {
        self.open(input)
    }
}

// ---------------------------------------------------------------------------
// CapabilityService — shared registry, Arc-shared
// ---------------------------------------------------------------------------

/// Shared registry of every registered capability. Cloning is cheap and
/// shares state, so main, plugins via cordis inject, and the HTTP bridge
/// all see the same registrations.
#[derive(Clone, Default)]
pub struct CapabilityService {
    by_name: Arc<Mutex<Vec<Arc<dyn AnyCapability>>>>,
}

impl CapabilityService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, cap: Arc<dyn AnyCapability>) -> Result<(), CapabilityError> {
        let mut by_name = self.by_name.lock().expect("capability service poisoned");
        let name = cap.meta().name.clone();
        if by_name.iter().any(|c| c.meta().name == name) {
            return Err(CapabilityError::AlreadyExists(name));
        }
        by_name.push(cap);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn AnyCapability>> {
        self.by_name
            .lock()
            .expect("capability service poisoned")
            .iter()
            .find(|c| c.meta().name == name)
            .cloned()
    }

    pub fn enumerate(&self) -> Vec<CapabilityMeta> {
        let mut metas: Vec<_> = self
            .by_name
            .lock()
            .expect("capability service poisoned")
            .iter()
            .map(|c| c.meta().clone())
            .collect();
        metas.sort_by(|a, b| a.name.cmp(&b.name));
        metas
    }
}

#[derive(Debug)]
pub enum CapabilityError {
    AlreadyExists(String),
}

impl std::fmt::Display for CapabilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyExists(n) => write!(f, "capability already registered: {n}"),
        }
    }
}

impl std::error::Error for CapabilityError {}

impl From<CapabilityError> for cordis::Error {
    fn from(e: CapabilityError) -> Self {
        cordis::Error::msg(e.to_string())
    }
}

// ---------------------------------------------------------------------------
// Manifest → Meta helper — used by the factory
// ---------------------------------------------------------------------------

pub fn meta_from_decl(
    id: CapabilityId,
    decl: &CapabilityDecl,
    plugin: &PluginId,
    budget: &CapabilityBudget,
) -> CapabilityMeta {
    CapabilityMeta {
        id,
        name: decl.name.clone(),
        plugin: plugin.clone(),
        in_type: decl.in_type.clone(),
        out_type: decl.out_type.clone(),
        streaming: decl.streaming,
        timeout_ms: budget.timeout_ms,
    }
}