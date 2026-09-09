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

/// Rights attached to a capability, supplied when deriving a child via
/// `grant`, `transfer`, or `restrict`. The child capability keeps the
/// same `R` (resource type) and `K` (kind) as the source, but with
/// reduced (or equal) rights.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapabilityRights {
    /// Wall-clock timeout per call. Smaller = more restrictive.
    pub timeout_ms: u32,
}

impl Default for CapabilityRights {
    fn default() -> Self {
        Self { timeout_ms: 5000 }
    }
}

impl CapabilityRights {
    pub fn with_timeout(mut self, ms: u32) -> Self {
        self.timeout_ms = ms;
        self
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

    /// Current rights of this capability.
    pub fn rights(&self) -> CapabilityRights {
        CapabilityRights { timeout_ms: self.budget.timeout_ms }
    }

    /// Derive a new capability sharing the same handler, with the given
    /// rights and a fresh `CapabilityId`. Used by `grant`, `transfer`,
    /// and `restrict`.
    pub fn derive(&self, rights: CapabilityRights, new_id: CapabilityId) -> Self {
        let new_budget = Arc::new(CapabilityBudget::new(rights.timeout_ms));
        let mut new_meta = self.meta.clone();
        new_meta.id = new_id;
        new_meta.timeout_ms = rights.timeout_ms;
        Self::new_typed(new_meta, self.handler.clone(), new_budget)
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

    /// **Revoke**: clear this slot. Convenience wrapper around
    /// `cspace.revoke(self.id)`.
    pub fn revoke(&self) -> bool {
        self.space.revoke(self.id)
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

    /// **Grant**: derive a new sync slot with reduced rights; source
    /// preserved. seL4: CNode.Mint.
    pub fn grant(&self, rights: CapabilityRights, new_name: String) -> Result<SlotId, CapabilityError> {
        self.space.grant_sync::<R>(self.id, rights, new_name)
    }

    /// **Restrict**: same as `grant` — derive a new sync slot with
    /// reduced rights; source preserved.
    pub fn restrict(&self, rights: CapabilityRights, new_name: String) -> Result<SlotId, CapabilityError> {
        self.space.restrict_sync::<R>(self.id, rights, new_name)
    }

    /// **Transfer**: move the sync capability to a fresh slot. Source
    /// slot is cleared. seL4: CNode.Move.
    pub fn transfer(&self, rights: CapabilityRights) -> Result<SlotId, CapabilityError> {
        self.space.transfer_sync::<R>(self.id, rights)
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

    /// **Grant**: derive a new streaming slot with reduced rights.
    pub fn grant(&self, rights: CapabilityRights, new_name: String) -> Result<SlotId, CapabilityError> {
        self.space.grant_stream::<R>(self.id, rights, new_name)
    }

    /// **Restrict**: derive a new streaming slot with reduced rights.
    pub fn restrict(&self, rights: CapabilityRights, new_name: String) -> Result<SlotId, CapabilityError> {
        self.space.restrict_stream::<R>(self.id, rights, new_name)
    }

    /// **Transfer**: move the streaming capability to a fresh slot.
    pub fn transfer(&self, rights: CapabilityRights) -> Result<SlotId, CapabilityError> {
        self.space.transfer_stream::<R>(self.id, rights)
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
    next_derived: AtomicU64,
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
                next_derived: AtomicU64::new(0),
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

    /// Snapshot of every occupied slot's metadata, sorted by name.
    pub fn enumerate(&self) -> Vec<CapabilityMeta> {
        let slots = self.inner.slots.read().expect("cspace poisoned");
        let mut metas: Vec<_> = slots.values().map(|e| e.erased.meta().clone()).collect();
        metas.sort_by(|a, b| a.name.cmp(&b.name));
        metas
    }

    // -----------------------------------------------------------------------
    // Capability operations: grant / transfer / restrict / revoke
    //
    // seL4 analogue:
    //   grant    — CNode.Mint, parent preserved
    //   transfer — CNode.Move, source cleared
    //   restrict — CNode.Mutate (rights reduction), source preserved
    //   revoke   — CNode.Delete + Revoke
    //
    // All four operations work on a typed slot the caller already holds.
    // The derived capabilities share the underlying handler `Arc<R>` with
    // the source — so the same resource is reused.
    // -----------------------------------------------------------------------

    /// Mint a fresh `CapabilityId` for a derived capability.
    fn next_derived_id(&self) -> CapabilityId {
        let raw = self.inner.next_derived.fetch_add(1, Ordering::Relaxed) + 1;
        // Derived caps share the high bits with their parent concept; we
        // tag the high bit so it's recognizable in logs.
        CapabilityId(raw | (1u64 << 62))
    }

    /// Internal: insert at a freshly-allocated slot under a given name.
    fn install_at(
        &self,
        erased: Arc<dyn AnyCapability>,
        typed: Arc<dyn Any + Send + Sync>,
        new_name: String,
    ) -> SlotId {
        let new_slot = self.allocate();
        let mut slots = self.inner.slots.write().expect("cspace poisoned");
        let prev = slots.insert(new_slot, SlotEntry { erased, typed });
        let mut names = self.inner.names.write().expect("cspace poisoned");
        if let Some(prev_entry) = prev {
            names.retain(|_, s| *s != new_slot);
            drop(prev_entry);
        }
        names.insert(new_name, new_slot);
        new_slot
    }

    /// **Grant** (sync). Derive a new sync slot with the given rights;
    /// source slot is unchanged. New slot is registered under `new_name`.
    pub fn grant_sync<R>(
        &self,
        from: SlotId,
        rights: CapabilityRights,
        new_name: String,
    ) -> Result<SlotId, CapabilityError>
    where
        R: SyncResource + 'static,
    {
        let source: Arc<Capability<R, SyncKind>> = self
            .lookup_typed::<R, SyncKind>(from)
            .ok_or(CapabilityError::SlotEmpty(from))?;
        let new_id = self.next_derived_id();
        let derived = source.derive(rights, new_id);
        let arc = Arc::new(derived);
        let erased: Arc<dyn AnyCapability> = arc.clone();
        let typed: Arc<dyn Any + Send + Sync> = arc;
        Ok(self.install_at(erased, typed, new_name))
    }

    /// **Grant** (stream). Derive a new streaming slot with the given
    /// rights; source unchanged.
    pub fn grant_stream<R>(
        &self,
        from: SlotId,
        rights: CapabilityRights,
        new_name: String,
    ) -> Result<SlotId, CapabilityError>
    where
        R: StreamResource + 'static,
    {
        let source: Arc<Capability<R, StreamKind>> = self
            .lookup_typed::<R, StreamKind>(from)
            .ok_or(CapabilityError::SlotEmpty(from))?;
        let new_id = self.next_derived_id();
        let derived = source.derive(rights, new_id);
        let arc = Arc::new(derived);
        let erased: Arc<dyn AnyCapability> = arc.clone();
        let typed: Arc<dyn Any + Send + Sync> = arc;
        Ok(self.install_at(erased, typed, new_name))
    }

    /// **Transfer** (sync). Move the capability to a fresh slot. New
    /// slot takes the source's name. Source slot is cleared.
    pub fn transfer_sync<R>(
        &self,
        from: SlotId,
        rights: CapabilityRights,
    ) -> Result<SlotId, CapabilityError>
    where
        R: SyncResource + 'static,
    {
        let source: Arc<Capability<R, SyncKind>> = self
            .lookup_typed::<R, SyncKind>(from)
            .ok_or(CapabilityError::SlotEmpty(from))?;
        let source_name = source.name().to_string();
        let new_id = self.next_derived_id();
        let derived = source.derive(rights, new_id);
        let arc = Arc::new(derived);
        let erased: Arc<dyn AnyCapability> = arc.clone();
        let typed: Arc<dyn Any + Send + Sync> = arc;
        let new_slot = self.install_at(erased, typed, source_name);
        self.revoke(from);
        Ok(new_slot)
    }

    /// **Transfer** (stream).
    pub fn transfer_stream<R>(
        &self,
        from: SlotId,
        rights: CapabilityRights,
    ) -> Result<SlotId, CapabilityError>
    where
        R: StreamResource + 'static,
    {
        let source: Arc<Capability<R, StreamKind>> = self
            .lookup_typed::<R, StreamKind>(from)
            .ok_or(CapabilityError::SlotEmpty(from))?;
        let source_name = source.name().to_string();
        let new_id = self.next_derived_id();
        let derived = source.derive(rights, new_id);
        let arc = Arc::new(derived);
        let erased: Arc<dyn AnyCapability> = arc.clone();
        let typed: Arc<dyn Any + Send + Sync> = arc;
        let new_slot = self.install_at(erased, typed, source_name);
        self.revoke(from);
        Ok(new_slot)
    }

    /// **Restrict** (sync) — derive a new sync slot with reduced rights.
    pub fn restrict_sync<R>(
        &self,
        from: SlotId,
        rights: CapabilityRights,
        new_name: String,
    ) -> Result<SlotId, CapabilityError>
    where
        R: SyncResource + 'static,
    {
        self.grant_sync::<R>(from, rights, new_name)
    }

    /// **Restrict** (stream) — derive a new streaming slot with reduced
    /// rights.
    pub fn restrict_stream<R>(
        &self,
        from: SlotId,
        rights: CapabilityRights,
        new_name: String,
    ) -> Result<SlotId, CapabilityError>
    where
        R: StreamResource + 'static,
    {
        self.grant_stream::<R>(from, rights, new_name)
    }

    /// **Revoke**: clear a slot. The slot id remains valid (stale
    /// `Slot<R, K>` references don't panic), but `capability()` /
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
}
#[derive(Debug)]
pub enum CapabilityError {
    AlreadyExists(String),
    /// The slot was empty or revoked when an operation tried to read it.
    SlotEmpty(SlotId),
}

impl std::fmt::Display for CapabilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyExists(n) => write!(f, "capability already installed: {n}"),
            Self::SlotEmpty(s) => write!(f, "slot {} empty or revoked", s.raw()),
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