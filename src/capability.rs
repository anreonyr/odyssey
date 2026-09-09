//! Capability model — single `Resource` trait, single `Capability<R>`.
//!
//! ## Hierarchy
//!
//! - `CapabilitySpace` — the namespace (seL4 CSpace)
//! - `SlotId` — stable position in the space
//! - `Slot<R>` — typed reference to a slot (the unit of possession)
//! - `Capability<R>` — what occupies a slot
//!
//! A `Capability<R>` carries:
//! - `meta: CapabilityMeta` — id, name, types, timeout_ms
//! - `handler: Arc<R>` — the resource (R: Resource)
//! - `budget: Arc<CapabilityBudget>` — per-call wall-clock cap
//! - `kind: CapKind` — sync vs stream (runtime; see note below)
//!
//! ## Sync vs stream distinction
//!
//! The single `Resource` trait provides both `invoke` and `open` methods.
//! Default impls return `Err` with a clear "not a sync/streaming
//! capability" message; concrete resources override the one that
//! applies. The capability carries a runtime `CapKind` so the wrong
//! method call is caught early with a typed error message instead of
//! falling through to a generic "not implemented" panic.
//!
//! seL4 has type-level kinds via different kernel object types; here
//! we trade compile-time distinction for a uniform public API.
//!
//! ## Possession
//!
//! Plugins hold `Slot<R>` (unforgeable). The CSpace can revoke slot
//! contents; the slot reference itself stays valid but lookups fail.

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
// Kinds
// ---------------------------------------------------------------------------

/// Runtime distinction between sync and streaming capabilities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapKind {
    Sync,
    Stream,
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

/// Rights attached to a capability. Supplied when deriving a child via
/// `grant`, `transfer`, or `restrict`. Currently a single dimension
/// (`timeout_ms`); extensible to rights bits later.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapabilityRights {
    pub timeout_ms: u32,
}

impl Default for CapabilityRights {
    fn default() -> Self {
        Self { timeout_ms: 5000 }
    }
}

impl CapabilityRights {
    #[allow(dead_code)]
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

// ---------------------------------------------------------------------------
// Resource trait — both invoke and open, with default impls
// ---------------------------------------------------------------------------

/// Plugin module's resource. Implements both `invoke` (for sync) and
/// `open` (for streaming). Default impls return an `Err` so a plugin
/// only needs to override the one that applies.
pub trait Resource: Send + Sync + 'static {
    fn invoke(&self, _input: Value) -> Result<Value, String> {
        Err("not a sync capability".into())
    }
    fn open(&self, _input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String> {
        Err("not a streaming capability".into())
    }
}

// ---------------------------------------------------------------------------
// Capability<R> — typed capability handle
// ---------------------------------------------------------------------------

/// Unforgeable capability object. Wraps an `Arc<R>` (the resource) with
/// metadata, budget, and a runtime `CapKind`.
pub struct Capability<R: Resource> {
    meta: CapabilityMeta,
    handler: Arc<R>,
    budget: Arc<CapabilityBudget>,
    kind: CapKind,
}

impl<R: Resource> Capability<R> {
    pub(crate) fn new(
        meta: CapabilityMeta,
        handler: Arc<R>,
        budget: Arc<CapabilityBudget>,
        kind: CapKind,
    ) -> Self {
        Self { meta, handler, budget, kind }
    }

    #[allow(dead_code)]
    pub fn meta(&self) -> &CapabilityMeta {
        &self.meta
    }
    pub fn name(&self) -> &str {
        &self.meta.name
    }
    pub fn id(&self) -> CapabilityId {
        self.meta.id.clone()
    }
    #[allow(dead_code)]
    pub fn kind(&self) -> CapKind {
        self.kind
    }
    pub fn rights(&self) -> CapabilityRights {
        CapabilityRights { timeout_ms: self.budget.timeout_ms }
    }

    /// Derive a new capability sharing the same handler `Arc<R>` with the
    /// source, with the given rights and a fresh `CapabilityId`. Kind is
    /// preserved.
    pub fn derive(&self, rights: CapabilityRights, new_id: CapabilityId) -> Self {
        let new_budget = Arc::new(CapabilityBudget::new(rights.timeout_ms));
        let mut new_meta = self.meta.clone();
        new_meta.id = new_id;
        new_meta.timeout_ms = rights.timeout_ms;
        Self {
            meta: new_meta,
            handler: self.handler.clone(),
            budget: new_budget,
            kind: self.kind,
        }
    }
}

impl<R: Resource> Clone for Capability<R> {
    fn clone(&self) -> Self {
        Self {
            meta: self.meta.clone(),
            handler: self.handler.clone(),
            budget: self.budget.clone(),
            kind: self.kind,
        }
    }
}

impl<R: Resource> Capability<R> {
    /// Invoke the resource. If elapsed wall-clock time exceeds the
    /// token's `timeout_ms`, returns `Err` and the handler result is
    /// dropped.
    pub fn invoke(&self, input: Value) -> Result<Value, String> {
        if self.kind != CapKind::Sync {
            return Err(format!(
                "{}: not a sync capability",
                self.meta.name
            ));
        }
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

    /// Open the stream. Per-chunk delivery is the resource's job; the
    /// budget governs the open-to-last-chunk window for the caller.
    pub fn open(&self, input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String> {
        if self.kind != CapKind::Stream {
            return Err(format!(
                "{}: not a streaming capability",
                self.meta.name
            ));
        }
        self.handler.open(input)
    }
}

// ---------------------------------------------------------------------------
// AnyCapability — erased view, downcastable to Capability<R>
// ---------------------------------------------------------------------------

/// Erased capability: lets heterogeneous `Capability<R>` values coexist in
/// a single registry. Provides `as_any` for downcasting to a typed
/// `&dyn Any` reference; the caller can then re-`Arc::new` the cloned
/// capability (which is `Clone`).
pub trait AnyCapability: Any + Send + Sync {
    fn meta(&self) -> &CapabilityMeta;
    fn is_streaming(&self) -> bool;
    fn invoke_dyn(&self, input: Value) -> Result<Value, String>;
    fn open_dyn(&self, input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String>;
    fn as_any(&self) -> &dyn Any;
}

impl<R: Resource> AnyCapability for Capability<R> {
    fn meta(&self) -> &CapabilityMeta {
        &self.meta
    }
    fn is_streaming(&self) -> bool {
        self.kind == CapKind::Stream
    }
    fn invoke_dyn(&self, input: Value) -> Result<Value, String> {
        self.invoke(input)
    }
    fn open_dyn(&self, input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String> {
        self.open(input)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

// ---------------------------------------------------------------------------
// CapabilitySpace — namespace of slots
// ---------------------------------------------------------------------------

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
    /// Single field: Arc<dyn AnyCapability> is both the erased view AND the
    /// typed downcast target (AnyCapability: Any).
    cap: Arc<dyn AnyCapability>,
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

    /// Install a typed capability into a slot, indexed under the cap's
    /// own `name` in the name index.
    pub fn install<R: Resource>(
        &self,
        slot: SlotId,
        cap: Arc<Capability<R>>,
    ) {
        let name = cap.name().to_string();
        let erased: Arc<dyn AnyCapability> = cap;
        let mut slots = self.inner.slots.write().expect("cspace poisoned");
        let prev = slots.insert(slot, SlotEntry { cap: erased });
        let mut names = self.inner.names.write().expect("cspace poisoned");
        if let Some(prev_entry) = prev {
            names.retain(|_, s| *s != slot);
            drop(prev_entry);
        }
        names.insert(name, slot);
    }

    /// Typed lookup. The caller must know `R`; otherwise returns `None`.
    pub fn lookup_typed<R: Resource>(&self, slot: SlotId) -> Option<Arc<Capability<R>>> {
        let cap = self
            .inner
            .slots
            .read()
            .expect("cspace poisoned")
            .get(&slot)
            .map(|e| e.cap.clone())?;
        let concrete = cap.as_any().downcast_ref::<Capability<R>>()?;
        Some(Arc::new(concrete.clone()))
    }

    /// Erased lookup. Used by HTTP bridge and pipeline.
    pub fn lookup_erased(&self, slot: SlotId) -> Option<Arc<dyn AnyCapability>> {
        self.inner
            .slots
            .read()
            .expect("cspace poisoned")
            .get(&slot)
            .map(|e| e.cap.clone())
    }

    /// Look up by capability name (HTTP bridge / external API).
    pub fn lookup_by_name(&self, name: &str) -> Option<Arc<dyn AnyCapability>> {
        let slot = *self
            .inner
            .names
            .read()
            .expect("cspace poisoned")
            .get(name)?;
        self.lookup_erased(slot)
    }

    /// Capability metadata at a slot.
    pub fn slot_meta(&self, slot: SlotId) -> Option<CapabilityMeta> {
        self.inner
            .slots
            .read()
            .expect("cspace poisoned")
            .get(&slot)
            .map(|e| e.cap.meta().clone())
    }

    /// Snapshot of every occupied slot's metadata, sorted by name.
    pub fn enumerate(&self) -> Vec<CapabilityMeta> {
        let slots = self.inner.slots.read().expect("cspace poisoned");
        let mut metas: Vec<_> = slots.values().map(|e| e.cap.meta().clone()).collect();
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
    // -----------------------------------------------------------------------

    /// Mint a fresh `CapabilityId` for a derived capability. The high bit
    /// marks it as derived.
    fn next_derived_id(&self) -> CapabilityId {
        let raw = self.inner.next_derived.fetch_add(1, Ordering::Relaxed) + 1;
        CapabilityId(raw | (1u64 << 62))
    }

    fn install_derived<R: Resource>(
        &self,
        new_cap: Capability<R>,
        new_name: String,
    ) -> SlotId
    where
        R: Resource,
    {
        let new_slot = self.allocate();
        let cap: Arc<dyn AnyCapability> = Arc::new(new_cap);
        let mut slots = self.inner.slots.write().expect("cspace poisoned");
        let prev = slots.insert(new_slot, SlotEntry { cap });
        let mut names = self.inner.names.write().expect("cspace poisoned");
        if let Some(prev_entry) = prev {
            names.retain(|_, s| *s != new_slot);
            drop(prev_entry);
        }
        names.insert(new_name, new_slot);
        new_slot
    }

    /// **Grant**: derive a new slot with the given rights; source slot
    /// is unchanged. New slot is registered under `new_name`.
    pub fn grant<R: Resource>(
        &self,
        from: SlotId,
        rights: CapabilityRights,
        new_name: String,
    ) -> Result<SlotId, CapabilityError> {
        let source: Arc<Capability<R>> = self
            .lookup_typed::<R>(from)
            .ok_or(CapabilityError::SlotEmpty(from))?;
        let new_id = self.next_derived_id();
        let derived = source.derive(rights, new_id);
        Ok(self.install_derived(derived, new_name))
    }

    /// **Transfer**: move the capability to a fresh slot with the given
    /// rights. New slot takes the source's name. Source slot is cleared.
    pub fn transfer<R: Resource>(
        &self,
        from: SlotId,
        rights: CapabilityRights,
    ) -> Result<SlotId, CapabilityError> {
        let source: Arc<Capability<R>> = self
            .lookup_typed::<R>(from)
            .ok_or(CapabilityError::SlotEmpty(from))?;
        let source_name = source.name().to_string();
        let new_id = self.next_derived_id();
        let derived = source.derive(rights, new_id);
        let new_slot = self.install_derived(derived, source_name);
        self.revoke(from);
        Ok(new_slot)
    }

    /// **Restrict**: same as `grant` — derive a new slot with reduced
    /// rights, source preserved. Provided as a distinct method so the
    /// call site documents intent.
    pub fn restrict<R: Resource>(
        &self,
        from: SlotId,
        rights: CapabilityRights,
        new_name: String,
    ) -> Result<SlotId, CapabilityError> {
        self.grant::<R>(from, rights, new_name)
    }

    /// **Revoke**: clear a slot. The slot id remains valid (stale
    /// `Slot<R>` references don't panic), but `capability()` /
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
// Slot<R> — typed reference to a slot
// ---------------------------------------------------------------------------

/// Typed, unforgeable reference to a slot. The unit of possession.
pub struct Slot<R: Resource> {
    space: CapabilitySpace,
    id: SlotId,
    _phantom: PhantomData<R>,
}

impl<R: Resource> Slot<R> {
    pub fn new(space: CapabilitySpace, id: SlotId) -> Self {
        Self { space, id, _phantom: PhantomData }
    }

    pub fn id(&self) -> SlotId {
        self.id
    }

    #[allow(dead_code)]
    pub fn space(&self) -> &CapabilitySpace {
        &self.space
    }

    /// The capability currently occupying this slot, if any. Returns
    /// `None` when the slot has been revoked or is empty.
    pub fn capability(&self) -> Option<Arc<Capability<R>>> {
        self.space.lookup_typed::<R>(self.id)
    }

    /// Capability metadata at this slot.
    pub fn meta(&self) -> Option<CapabilityMeta> {
        self.space.slot_meta(self.id)
    }

    /// Run-time kind of the cap at this slot, if any.
    #[allow(dead_code)]
    pub fn kind(&self) -> Option<CapKind> {
        self.capability().map(|c| c.kind())
    }

    /// Direct sync invocation via the slot.
    pub fn invoke(&self, input: Value) -> Result<Value, String> {
        let cap = self
            .capability()
            .ok_or_else(|| format!("slot {} empty or revoked", self.id.raw()))?;
        cap.invoke(input)
    }

    /// Direct stream open via the slot.
    pub fn open(&self, input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String> {
        let cap = self
            .capability()
            .ok_or_else(|| format!("slot {} empty or revoked", self.id.raw()))?;
        cap.open(input)
    }

    /// **Grant**: derive a new slot with reduced rights; source preserved.
    /// seL4: CNode.Mint.
    pub fn grant(
        &self,
        rights: CapabilityRights,
        new_name: String,
    ) -> Result<SlotId, CapabilityError> {
        self.space.grant::<R>(self.id, rights, new_name)
    }

    /// **Restrict**: same as `grant` with intent distinction — derive a
    /// more limited view of your own capability.
    pub fn restrict(
        &self,
        rights: CapabilityRights,
        new_name: String,
    ) -> Result<SlotId, CapabilityError> {
        self.space.restrict::<R>(self.id, rights, new_name)
    }

    /// **Transfer**: move the capability to a fresh slot. Source cleared.
    /// seL4: CNode.Move.
    pub fn transfer(&self, rights: CapabilityRights) -> Result<SlotId, CapabilityError> {
        self.space.transfer::<R>(self.id, rights)
    }

    /// **Revoke**: clear this slot.
    pub fn revoke(&self) -> bool {
        self.space.revoke(self.id)
    }
}

impl<R: Resource> Clone for Slot<R> {
    fn clone(&self) -> Self {
        Self {
            space: self.space.clone(),
            id: self.id,
            _phantom: PhantomData,
        }
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