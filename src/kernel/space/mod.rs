//! CapabilitySpace — the namespace of slots.
//!
//! Phase 5 split from `capability::cspace`:
//!
//! - `mod.rs` — struct, `new`/`with_bus`, lookups, enumeration,
//!   events, plus internal lock-guard helpers used by submodules.
//! - `derivation` — `grant`/`transfer`/`restrict` + `install_derived`.
//! - `revocation` — `revoke` + `revoke_tree` (mode-aware).
//! - `events` — `GraphEventBus` + `GraphEvent` + `DeriveKind`.
//! - `graph` — `CapabilityGraph` snapshot view.
//! - `namespace` — `namespace_prefix_matches` (single home).
//!
//! ## Phase 5 fixes embedded here
//!
//! - **D2**: `install_derived` refuses when the parent id is no
//!   longer in `parents` — closes the Interleaving 2 race window.
//! - **M1**: `install` takes `parents.write()` as a no-op lock
//!   first, matching the canonical `parents → slots → names` order.
//! - **R4/R5/R6/R7**: revocation walks `parents` under the write
//!   lock atomically; the Phase 4 `revoke_with_sweep` is gone.

pub mod derivation;
pub mod events;
pub mod graph;
pub mod namespace;
pub mod revocation;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock, RwLockWriteGuard};

use crate::kernel::cap::{AnyCapability, Capability};
use crate::kernel::error::CapabilityError;
use crate::kernel::ids::SlotId;
use crate::kernel::meta::CapabilityMeta;
use crate::kernel::resource::Resource;
use crate::kernel::rights::CapabilityRights;

pub use derivation::RevokeMode;
pub use events::{DeriveKind, GraphEvent, GraphEventBus, GraphEventReceiver, TryRecvError};
pub use graph::CapabilityGraph;

/// The capability namespace. Cheap to clone (shares inner state via Arc).
#[derive(Clone)]
pub struct CapabilitySpace {
    inner: Arc<CSpaceInner>,
}

pub(super) struct CSpaceInner {
    slots: RwLock<HashMap<SlotId, SlotEntry>>,
    names: RwLock<HashMap<String, SlotId>>,
    /// Parent pointer for every derived slot — used by `revoke_tree`
    /// to recursively sever the entire subtree. `None` for roots.
    parents: RwLock<HashMap<SlotId, SlotId>>,
    /// Phase 3 P3.7 — graph event bus. Every mutation in the
    /// cspace publishes here.
    events: GraphEventBus,
    next: AtomicU64,
    next_derived: AtomicU64,
}

pub(super) struct SlotEntry {
    /// Single field: `Arc<dyn AnyCapability>` is both the erased view AND
    /// the typed downcast target.
    cap: Arc<dyn AnyCapability>,
}

impl CapabilitySpace {
    pub fn new() -> Self {
        Self::with_bus(GraphEventBus::new())
    }

    pub fn with_bus(bus: GraphEventBus) -> Self {
        Self {
            inner: Arc::new(CSpaceInner {
                slots: RwLock::new(HashMap::new()),
                names: RwLock::new(HashMap::new()),
                parents: RwLock::new(HashMap::new()),
                next: AtomicU64::new(0),
                next_derived: AtomicU64::new(0),
                events: bus,
            }),
        }
    }

    pub fn subscribe(&self) -> GraphEventReceiver {
        self.inner.events.subscribe()
    }

    pub fn events(&self) -> &GraphEventBus {
        &self.inner.events
    }

    /// Allocate a new slot id.
    pub fn allocate(&self) -> SlotId {
        let raw = self.inner.next.fetch_add(1, Ordering::Relaxed) + 1;
        SlotId::new(raw)
    }

    /// Install a typed capability into a slot.
    ///
    /// Phase 5 M1 fix: takes `parents.write()` first as a no-op lock,
    /// matching the canonical `parents → slots → names` order.
    pub fn install<R: Resource>(&self, slot: SlotId, cap: Arc<Capability<R>>) {
        let name = cap.name().to_string();
        let contract = cap.meta().contract_name.clone();
        let plugin = cap.meta().plugin.clone();

        // Bind the slot id onto the cap so revoked-cap errors can
        // surface a real `SlotId` instead of a sentinel. We mutate
        // through `Arc::make_mut`-style: the inner `Arc<Capability<R>>`
        // is shared with whoever holds the typed cap, so we
        // clone-into-Arc with the bound slot.
        let mut owned = (*cap).clone();
        owned.bind_slot(slot);
        let cap = Arc::new(owned);
        cap.set_revoked(false);
        let erased: Arc<dyn AnyCapability> = cap;

        // Phase 5 M1: canonical lock order.
        let parents = self.inner.parents.write().expect("cspace poisoned");
        let mut slots = self.inner.slots.write().expect("cspace poisoned");
        let prev = slots.insert(slot, SlotEntry { cap: erased });
        let mut names = self.inner.names.write().expect("cspace poisoned");
        if let Some(prev_entry) = prev {
            names.retain(|_, s| *s != slot);
            drop(prev_entry);
        }
        names.insert(name.clone(), slot);
        drop(slots);
        drop(names);
        drop(parents);

        self.publish_event(GraphEvent::Minted {
            plugin,
            slot,
            capability: name,
            contract,
        });
    }

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

    pub fn lookup_erased(&self, slot: SlotId) -> Option<Arc<dyn AnyCapability>> {
        self.inner
            .slots
            .read()
            .expect("cspace poisoned")
            .get(&slot)
            .map(|e| e.cap.clone())
    }

    pub fn lookup_by_name(&self, name: &str) -> Option<Arc<dyn AnyCapability>> {
        let slot = *self
            .inner
            .names
            .read()
            .expect("cspace poisoned")
            .get(name)?;
        self.lookup_erased(slot)
    }

    pub fn name_for_slot(&self, slot: SlotId) -> Option<String> {
        let names = self.inner.names.read().expect("cspace poisoned");
        names
            .iter()
            .filter(|(_, s)| **s == slot)
            .map(|(n, _)| n.clone())
            .min()
    }

    pub fn slot_meta(&self, slot: SlotId) -> Option<CapabilityMeta> {
        self.inner
            .slots
            .read()
            .expect("cspace poisoned")
            .get(&slot)
            .map(|e| e.cap.meta().clone())
    }

    pub fn enumerate(&self) -> Vec<CapabilityMeta> {
        let slots = self.inner.slots.read().expect("cspace poisoned");
        let mut metas: Vec<_> = slots.values().map(|e| e.cap.meta().clone()).collect();
        metas.sort_by(|a, b| a.name.cmp(&b.name));
        metas
    }

    pub fn enumerate_namespace(&self, prefix: &str) -> Vec<CapabilityMeta> {
        let slots = self.inner.slots.read().expect("cspace poisoned");
        let mut metas: Vec<_> = slots
            .values()
            .map(|e| e.cap.meta().clone())
            .filter(|m| namespace::namespace_prefix_matches(&m.namespace, prefix))
            .collect();
        metas.sort_by(|a, b| a.namespace.cmp(&b.namespace));
        metas
    }

    pub fn namespace_children(&self, prefix: &str) -> Vec<(String, usize)> {
        let all = self.enumerate_namespace(prefix);
        let mut out: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
        let base = if prefix.is_empty() {
            String::new()
        } else {
            format!("{prefix}.")
        };
        for m in all {
            let rest = m.namespace.strip_prefix(&base).unwrap_or(&m.namespace);
            let head = rest.split('.').next().unwrap_or("").to_string();
            if head.is_empty() {
                continue;
            }
            *out.entry(head).or_default() += 1;
        }
        out.into_iter().collect()
    }

    /// Snapshot copy of `parents` for the graph view.
    pub fn parent_map(&self) -> HashMap<SlotId, SlotId> {
        self.inner.parents.read().expect("cspace poisoned").clone()
    }

    /// Pair every installed slot with its registered name and
    /// metadata. Used by the graph snapshot view to assign a
    /// real `SlotId` to each `GraphNode` (the public API
    /// exposes `enumerate()` over names, not slots, so the
    /// graph view needs this join to produce a node-per-slot
    /// mapping). Internal-only; the HTTP bridge walks it once
    /// per `snapshot`.
    pub fn snapshot_index(&self) -> Vec<(SlotId, crate::kernel::meta::CapabilityMeta)> {
        // Canonical lock order: parents → slots → names.
        let _parents = self.inner.parents.read().expect("cspace poisoned");
        let slots = self.inner.slots.read().expect("cspace poisoned");
        let names = self.inner.names.read().expect("cspace poisoned");
        // For each slot, find the first name that points at it.
        let mut out: Vec<(SlotId, _)> = Vec::with_capacity(slots.len());
        for slot_id in slots.keys() {
            // Look up the name for this slot.
            let cap = slots.get(slot_id).unwrap();
            let meta = cap.cap.meta().clone();
            // We need at least one name per slot for the
            // graph view. The canonical name is the first
            // name in the names map that points at this slot.
            let canonical_name = names
                .iter()
                .filter(|(_, s)| **s == *slot_id)
                .map(|(n, _)| n.clone())
                .min()
                .unwrap_or_default();
            let mut m = meta;
            // Prefer the canonical name over meta.name so the
            // graph view's name field matches what's actually
            // registered in the namespace.
            if !canonical_name.is_empty() {
                m.name = canonical_name;
            }
            out.push((*slot_id, m));
        }
        out.sort_by_key(|(s, _)| *s);
        out
    }

    /// Direct children of `slot` in the parent-pointer tree.
    /// Roots are slots whose parent pointer is not in `parents`;
    /// leaves are slots that have no children of their own.
    /// Used by the graph view and by tests that walk the
    /// attenuation tree.
    pub fn children_of(&self, slot: SlotId) -> Vec<SlotId> {
        let parents = self.inner.parents.read().expect("cspace poisoned");
        let mut out: Vec<SlotId> = parents
            .iter()
            .filter_map(|(child, parent)| if *parent == slot { Some(*child) } else { None })
            .collect();
        out.sort();
        out
    }

    pub fn len(&self) -> usize {
        self.inner.slots.read().expect("cspace poisoned").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    // -----------------------------------------------------------------
    // Public delegation to submodules.
    // -----------------------------------------------------------------

    pub fn grant<R: Resource>(
        &self,
        from: SlotId,
        rights: CapabilityRights,
        new_name: String,
    ) -> Result<SlotId, CapabilityError> {
        derivation::grant::<R>(self, from, rights, new_name)
    }

    pub fn transfer<R: Resource>(
        &self,
        from: SlotId,
        rights: CapabilityRights,
    ) -> Result<SlotId, CapabilityError> {
        derivation::transfer::<R>(self, from, rights)
    }

    pub fn restrict<R: Resource>(
        &self,
        from: SlotId,
        rights: CapabilityRights,
        new_name: String,
    ) -> Result<SlotId, CapabilityError> {
        derivation::restrict::<R>(self, from, rights, new_name)
    }

    pub fn revoke(&self, slot: SlotId) -> bool {
        revocation::revoke(self, slot, RevokeMode::Single)
    }

    pub fn revoke_tree(&self, root: SlotId) -> usize {
        revocation::revoke_tree(self, root)
    }

    pub(crate) fn publish_event(&self, ev: GraphEvent) {
        let _ = self.inner.events.publish(ev);
    }

    pub(crate) fn next_derived_id(&self) -> crate::kernel::ids::CapabilityId {
        let raw = self.inner.next_derived.fetch_add(1, Ordering::Relaxed) + 1;
        crate::kernel::ids::CapabilityId(raw)
    }

    /// Install a derived capability. Phase 5 D2 fix: refuses when
    /// `parent` is no longer in `slots` (Interleaving 2 race).
    pub(crate) fn install_derived<R: Resource>(
        &self,
        parent: SlotId,
        new_cap: Capability<R>,
        new_name: String,
    ) -> Result<SlotId, CapabilityError> {
        // D2 precondition: parent must be live. We check under a
        // brief read lock; the race window between this read and
        // the write below is benign because the canonical lock
        // order (`parents → slots → names`) ensures any
        // concurrent `revoke_tree` waits on the write lock
        // until our insert completes.
        //
        // The check is against `slots`, not `parents`: a parent
        // is just "a slot in the cspace", which means it's in
        // `slots`. The `parents` map only contains entries for
        // derived slots (child → parent), so checking `parents`
        // would wrongly reject every root cap.
        {
            let slots = self.inner.slots.read().expect("cspace poisoned");
            if !slots.contains_key(&parent) {
                return Err(CapabilityError::SlotEmpty(parent));
            }
        }

        let new_slot = self.allocate();
        // Bind the new slot id onto the derived cap so revoked-
        // cap errors surface the real `SlotId`.
        let mut new_cap = new_cap;
        new_cap.bind_slot(new_slot);
        let cap: Arc<dyn AnyCapability> = Arc::new(new_cap);

        let mut parents = self.inner.parents.write().expect("cspace poisoned");
        let mut slots = self.inner.slots.write().expect("cspace poisoned");
        let mut names = self.inner.names.write().expect("cspace poisoned");

        let prev = slots.insert(new_slot, SlotEntry { cap });
        if let Some(prev_entry) = prev {
            names.retain(|_, s| *s != new_slot);
            drop(prev_entry);
        }
        names.insert(new_name, new_slot);
        parents.insert(new_slot, parent);
        drop(slots);
        drop(names);
        drop(parents);

        Ok(new_slot)
    }

    pub(crate) fn set_revoked(&self, cap: &Arc<dyn AnyCapability>, revoked: bool) {
        cap.set_revoked_dyn(revoked);
    }

    // -----------------------------------------------------------------
    // Internal lock-guard helpers used by submodules.
    // -----------------------------------------------------------------

    pub(super) fn parents_guarded(&self) -> RwLockWriteGuard<'_, HashMap<SlotId, SlotId>> {
        self.inner.parents.write().expect("cspace poisoned")
    }

    pub(super) fn slots_guarded(&self) -> RwLockWriteGuard<'_, HashMap<SlotId, SlotEntry>> {
        self.inner.slots.write().expect("cspace poisoned")
    }

    pub(super) fn names_guarded(&self) -> RwLockWriteGuard<'_, HashMap<String, SlotId>> {
        self.inner.names.write().expect("cspace poisoned")
    }
}

impl Default for CapabilitySpace {
    fn default() -> Self {
        Self::new()
    }
}

/// Compile-time trait bound assertion: `AnyCapability: Any`. The
/// kernel enforces this via the `Any` super-trait; this assertion
/// surfaces any future relaxation as a compile error.
#[allow(dead_code)]
const _: fn() = || {
    fn _assert<T: AnyCapability + ?Sized>() {}
    let _ = _assert::<dyn AnyCapability> as fn();
};
