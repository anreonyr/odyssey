//! Capability model — seL4-inspired, three-tier: Space → Slot → Capability.
//!
//! ## Hierarchy
//!
//! - **`CapabilitySpace`** — the namespace (analogous to seL4 CSpace). Holds
//!   slots; supports allocate / lookup / install / revoke. Cloning is cheap
//!   and shares state, so the host and any code path that needs to read
//!   the namespace see the same slots.
//!
//! - **`Slot<R, K>`** — a typed, unforgeable reference to a position in a
//!   `CapabilitySpace`. The unit of possession. A plugin holding a
//!   `Slot<EchoResource, SyncKind>` can look up the capability currently
//!   occupying that slot, or invoke through it directly. The host can
//!   `revoke` the slot — clearing its contents — even while the plugin
//!   still holds the Slot reference.
//!
//! - **`Capability<R, K>`** — the actual capability object occupying a
//!   slot (analogous to seL4 capability + rights bits). Generic over
//!   `R` (the resource type) and `K` (sync vs streaming kind).
//!
//! ## Possession semantics
//!
//! seL4: a thread possesses capabilities through its CSpace. The kernel
//! can revoke a capability by clearing the slot, even while the thread
//! still references the slot.
//!
//! odyssey: a plugin possesses capabilities through a `Slot<R, K>`
//! reference. The host can revoke by `cspace.revoke(slot_id)` — the
//! plugin's `slot.capability()` then returns `None`, and direct
//! `slot.invoke()` returns `Err`.
//!
//! ## Type erasure
//!
//! For heterogeneous capabilities in one CSpace, we store both an erased
//! view (`Arc<dyn AnyCapability>`) for HTTP-bridge-style lookup and a
//! type-erased typed view (`Arc<dyn Any + Send + Sync>`) for typed
//! `Arc::downcast` when the caller knows `R, K`.

use std::any::Any;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;

use serde_json::Value;
use tokio::sync::mpsc;

use crate::manifest::{CapabilityDecl, PluginId};

// ---------------------------------------------------------------------------
// Kinds — sync vs stream, encoded as type-level PhantomData
// ---------------------------------------------------------------------------

pub struct SyncKind;
pub struct StreamKind;

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

pub trait SyncResource: Send + Sync + 'static {
    fn invoke(&self, input: Value) -> Result<Value, String>;
}

pub trait StreamResource: Send + Sync + 'static {
    fn open(&self, input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String>;
}

// ---------------------------------------------------------------------------
// Capability<R, K> — what occupies a slot
// ---------------------------------------------------------------------------

/// Unforgeable capability object. Wraps an `Arc<R>` (the actual handler)
/// with metadata and a per-call wall-clock budget. Generic over the
/// resource type `R` and the kind `K`.
pub struct Capability<R: Send + Sync + 'static, K: CapabilityKind = SyncKind> {
    meta: CapabilityMeta,
    handler: Arc<R>,
    budget: Arc<CapabilityBudget>,
    _kind: PhantomData<K>,
}

impl<R: Send + Sync + 'static, K: CapabilityKind> Capability<R, K> {
    pub(crate) fn new_typed(
        meta: CapabilityMeta,
        handler: Arc<R>,
        budget: Arc<CapabilityBudget>,
    ) -> Self {
        Self { meta, handler, budget, _kind: PhantomData }
    }

    pub fn name(&self) -> &str {
        &self.meta.name
    }

    pub fn id(&self) -> CapabilityId {
        self.meta.id.clone()
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
    pub fn open(&self, input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String> {
        self.handler.open(input)
    }
}

// ---------------------------------------------------------------------------
// AnyCapability — erased view (HTTP bridge, pipeline)
// ---------------------------------------------------------------------------

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
// CapabilitySpace — namespace of slots
// ---------------------------------------------------------------------------

/// Identifier for a slot position in a `CapabilitySpace`. Stable for the
/// lifetime of the space; can be copied.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SlotId(NonZeroU64);

impl SlotId {
    pub fn raw(&self) -> u64 {
        self.0.get()
    }
}

impl std::fmt::Display for SlotId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "slot:{}", self.0)
    }
}

/// A typed reference to a slot. The unit of possession.
///
/// `Slot<R, K>` says: "this position in the CSpace is permitted to hold
/// `Capability<R, K>`". The CSpace can revoke the contents; the Slot
/// reference itself stays valid (unforgeable) but `capability()` /
/// `invoke()` / `open()` will fail or return None.
pub struct Slot<R, K = SyncKind> {
    space: CapabilitySpace,
    id: SlotId,
    _phantom: PhantomData<(R, K)>,
}

impl<R, K> Slot<R, K>
where
    R: Send + Sync + 'static,
    K: CapabilityKind,
{
    pub fn new(space: CapabilitySpace, id: SlotId) -> Self {
        Self { space, id, _phantom: PhantomData }
    }

    pub fn id(&self) -> SlotId {
        self.id
    }

    /// The capability currently occupying this slot, if any. Returns `None`
    /// when the slot has been revoked or is empty.
    pub fn capability(&self) -> Option<Arc<Capability<R, K>>> {
        self.space.lookup_typed::<R, K>(self.id)
    }

    /// Capability metadata at this slot, regardless of `R, K`.
    pub fn meta(&self) -> Option<CapabilityMeta> {
        self.space.slot_meta(self.id)
    }
}

impl<R> Slot<R, SyncKind>
where
    R: SyncResource + 'static,
{
    /// Direct sync invocation via the slot.
    pub fn invoke(&self, input: Value) -> Result<Value, String> {
        let cap = self
            .capability()
            .ok_or_else(|| format!("slot {} empty or revoked", self.id.raw()))?;
        cap.invoke(input)
    }
}

impl<R> Slot<R, StreamKind>
where
    R: StreamResource + 'static,
{
    /// Direct stream open via the slot.
    pub fn open(&self, input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String> {
        let cap = self
            .capability()
            .ok_or_else(|| format!("slot {} empty or revoked", self.id.raw()))?;
        cap.open(input)
    }
}

impl<R, K> Clone for Slot<R, K> {
    fn clone(&self) -> Self {
        Self {
            space: self.space.clone(),
            id: self.id,
            _phantom: PhantomData,
        }
    }
}

/// The capability namespace. Cheap to clone (shares inner state via Arc).
#[derive(Clone)]
pub struct CapabilitySpace {
    inner: Arc<CSpaceInner>,
}

struct CSpaceInner {
    slots: RwLock<HashMap<SlotId, SlotEntry>>,
    names: RwLock<HashMap<String, SlotId>>,
    next: AtomicU64,
}

struct SlotEntry {
    /// Erased view for HTTP bridge / pipeline.
    erased: Arc<dyn AnyCapability>,
    /// Type-erased typed view for `Arc::downcast` from a typed Slot.
    typed: Arc<dyn Any + Send + Sync>,
}

impl Default for CapabilitySpace {
    fn default() -> Self {
        Self::new()
    }
}

impl CapabilitySpace {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(CSpaceInner {
                slots: RwLock::new(HashMap::new()),
                names: RwLock::new(HashMap::new()),
                next: AtomicU64::new(0),
            }),
        }
    }

    /// Allocate a new slot id. The slot is empty until `install` is called.
    pub fn allocate(&self) -> SlotId {
        let raw = self.inner.next.fetch_add(1, Ordering::Relaxed) + 1;
        SlotId(NonZeroU64::new(raw).unwrap())
    }

    /// Install a typed capability into a slot. Overwrites any previous
    /// occupant; updates the name index.
    pub fn install(
        &self,
        slot: SlotId,
        erased: Arc<dyn AnyCapability>,
        typed: Arc<dyn Any + Send + Sync>,
    ) {
        let name = erased.meta().name.clone();
        let mut slots = self.inner.slots.write().expect("cspace poisoned");
        let prev = slots.insert(slot, SlotEntry { erased, typed });
        let mut names = self.inner.names.write().expect("cspace poisoned");
        if let Some(prev_entry) = prev {
            names.retain(|_, s| *s != slot);
            drop(prev_entry);
        }
        names.insert(name, slot);
    }

    /// Erased lookup. Used by HTTP bridge and pipeline.
    pub fn lookup_erased(&self, slot: SlotId) -> Option<Arc<dyn AnyCapability>> {
        self.inner
            .slots
            .read()
            .expect("cspace poisoned")
            .get(&slot)
            .map(|e| e.erased.clone())
    }

    /// Typed lookup. The caller must know `R, K`; otherwise returns None.
    pub fn lookup_typed<R, K>(&self, slot: SlotId) -> Option<Arc<Capability<R, K>>>
    where
        R: Send + Sync + 'static,
        K: CapabilityKind,
    {
        let typed = self
            .inner
            .slots
            .read()
            .expect("cspace poisoned")
            .get(&slot)
            .map(|e| e.typed.clone())?;
        typed.downcast::<Capability<R, K>>().ok()
    }

    /// Look up a slot by capability name (HTTP bridge / external API).
    pub fn lookup_by_name(&self, name: &str) -> Option<Arc<dyn AnyCapability>> {
        let slot = *self.inner.names.read().expect("cspace poisoned").get(name)?;
        self.lookup_erased(slot)
    }

    /// Capability metadata at a slot, regardless of `R, K`.
    pub fn slot_meta(&self, slot: SlotId) -> Option<CapabilityMeta> {
        self.inner
            .slots
            .read()
            .expect("cspace poisoned")
            .get(&slot)
            .map(|e| e.erased.meta().clone())
    }

    /// Revoke a slot — drop the occupant. The slot id is still valid (so
    /// stale `Slot<R, K>` references don't panic), but `capability()` /
    /// `invoke()` / `open()` will fail.
    pub fn revoke(&self, slot: SlotId) -> bool {
        let mut slots = self.inner.slots.write().expect("cspace poisoned");
        let removed = slots.remove(&slot);
        if removed.is_some() {
            let mut names = self.inner.names.write().expect("cspace poisoned");
            names.retain(|_, s| *s != slot);
            true
        } else {
            false
        }
    }

    /// Snapshot of every occupied slot's metadata, sorted by name.
    pub fn enumerate(&self) -> Vec<CapabilityMeta> {
        let slots = self.inner.slots.read().expect("cspace poisoned");
        let mut metas: Vec<_> = slots.values().map(|e| e.erased.meta().clone()).collect();
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
            Self::AlreadyExists(n) => write!(f, "capability already installed: {n}"),
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
// Manifest → Meta helper
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