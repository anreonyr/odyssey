//! CapabilitySpace — the namespace of slots.
//!
//! Phase 8: merged the Phase 5 split (`mod.rs` + `derivation.rs` +
//! `revocation.rs` + `events.rs` + `namespace.rs`) into one
//! file. The `graph` module was dropped — `CapabilityGraph`
//! had zero production callers (only tests used it; the test
//! suite is being discarded). If a future introspection layer
//! needs the graph view, it can be re-added as a personality-side
//! observability module.
//!
//! The cspace holds:
//! - `slots: HashMap<SlotId, SlotEntry>` — the live capability table.
//! - `names: HashMap<String, SlotId>` — `lookup_by_name` index.
//! - `parents: HashMap<SlotId, SlotId>` — derivation parent pointers
//!   (used by `revoke_tree`).
//! - `events: GraphEventBus` — observability channel.
//! - two `AtomicU64` counters — id allocator + derived id allocator.
//!
//! Lock ordering invariant (canonical): `parents` → `slots` → `names`.
//! Every public method acquires them in this order to avoid
//! deadlock. `revoke_single` and `revoke_tree` hold the
//! `parents` write guard to serialise against concurrent grants.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock, RwLockWriteGuard};

use crate::core::identity::ids::{PluginId, SlotId};
use crate::core::meta::meta::CapabilityMeta;
use crate::core::rights::rights::CapabilityRights;

use crate::capability::handle::cap::Capability;
use crate::core::contract::resource::Resource;

// ---------------------------------------------------------------------------
// CapabilityEvent — kernel-side observability vocabulary
// ---------------------------------------------------------------------------

/// Default channel capacity. 256 events covers any plausible
/// boot or teardown sequence.
pub const DEFAULT_CAPACITY: usize = 256;

/// How a derived cap was produced from its parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeriveKind {
    /// `cspace.grant(...)` — derived slot is a peer's view of
    /// the parent. Source slot unchanged.
    Grant,
    /// `cspace.restrict(...)` — derived slot has strictly
    /// fewer rights than the parent. Source slot unchanged.
    Restrict,
    /// `cspace.transfer(...)` — source slot is cleared, the
    /// derived slot takes over its identity.
    Transfer,
}

/// One capability-graph mutation. Phase 8 split: lifecycle
/// events (`PluginActivated`, `PluginDeactivated`,
/// `ShutdownStarted`, `ShutdownCompleted`) moved out to
/// `personality::LifecycleEvent` because the kernel has no
/// knowledge of "plugins" or "shutdown" — those are
/// personality concerns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityEvent {
    Minted {
        plugin: PluginId,
        slot: SlotId,
        capability: String,
        contract: String,
    },
    Derived {
        parent: SlotId,
        child: SlotId,
        kind: DeriveKind,
    },
    Revoked {
        slot: SlotId,
        capability: Option<String>,
    },
    RevokeTree {
        root: SlotId,
        total: usize,
    },
}

pub type CapabilityEventReceiver = tokio::sync::broadcast::Receiver<CapabilityEvent>;
pub type TryRecvError = tokio::sync::broadcast::error::TryRecvError;

#[derive(Clone)]
pub struct GraphEventBus {
    tx: tokio::sync::broadcast::Sender<CapabilityEvent>,
}

impl GraphEventBus {
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let (tx, _rx) = tokio::sync::broadcast::channel(capacity.max(1));
        Self { tx }
    }

    pub fn subscribe(&self) -> CapabilityEventReceiver {
        self.tx.subscribe()
    }

    pub fn publish(&self, ev: CapabilityEvent) -> Result<usize, CapabilityEvent> {
        self.tx.send(ev).map_err(|e| e.0)
    }

    pub fn receiver_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

impl Default for GraphEventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for GraphEventBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraphEventBus")
            .field("receiver_count", &self.tx.receiver_count())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// RevokeMode — internal selector
// ---------------------------------------------------------------------------

/// Selector for revoke semantics. Phase 5 R6 fix: replaces the
/// Phase 4 `revoke` / private `revoke_with_sweep` split.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RevokeMode {
    /// Clear this slot only. Do not touch descendants. Used by
    /// `transfer` (the source slot, which has just been replaced
    /// by a freshly-installed derived slot).
    Single,
    /// Clear this slot and every descendant. Used by `revoke_tree`.
    Tree,
}

// ---------------------------------------------------------------------------
// Namespace helpers — single home for prefix matching
// ---------------------------------------------------------------------------

/// True when `namespace` is filed under `prefix` in the hierarchical
/// namespace tree. The empty prefix matches everything; a non-empty
/// prefix matches its own node and every descendant.
pub fn namespace_prefix_matches(namespace: &str, prefix: &str) -> bool {
    if prefix.is_empty() || prefix == "." {
        return true;
    }
    if namespace == prefix {
        return true;
    }
    namespace.starts_with(&format!("{prefix}."))
}

// ---------------------------------------------------------------------------
// CapabilitySpace — the namespace itself
// ---------------------------------------------------------------------------

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
    cap: Arc<dyn crate::capability::handle::cap::AnyCapability>,
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
                events: bus,
                next: AtomicU64::new(0),
                next_derived: AtomicU64::new(0),
            }),
        }
    }

    pub fn subscribe(&self) -> CapabilityEventReceiver {
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
        let erased: Arc<dyn crate::capability::handle::cap::AnyCapability> = cap;

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

        self.publish_event(CapabilityEvent::Minted {
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

    pub fn lookup_erased(
        &self,
        slot: SlotId,
    ) -> Option<Arc<dyn crate::capability::handle::cap::AnyCapability>> {
        self.inner
            .slots
            .read()
            .expect("cspace poisoned")
            .get(&slot)
            .map(|e| e.cap.clone())
    }

    pub fn lookup_by_name(
        &self,
        name: &str,
    ) -> Option<Arc<dyn crate::capability::handle::cap::AnyCapability>> {
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
            .filter(|m| namespace_prefix_matches(&m.namespace, prefix))
            .collect();
        metas.sort_by(|a, b| a.namespace.cmp(&b.namespace));
        metas
    }

    /// Snapshot copy of `parents` for the graph view.
    pub fn parent_map(&self) -> HashMap<SlotId, SlotId> {
        self.inner.parents.read().expect("cspace poisoned").clone()
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
    // Public delegation to derivation / revocation submodules.
    // -----------------------------------------------------------------

    pub fn grant<R: Resource>(
        &self,
        from: SlotId,
        rights: CapabilityRights,
        new_name: String,
    ) -> Result<SlotId, crate::capability::error::CapabilityError> {
        grant::<R>(self, from, rights, new_name)
    }

    pub fn transfer<R: Resource>(
        &self,
        from: SlotId,
        rights: CapabilityRights,
    ) -> Result<SlotId, crate::capability::error::CapabilityError> {
        transfer::<R>(self, from, rights)
    }

    pub fn restrict<R: Resource>(
        &self,
        from: SlotId,
        rights: CapabilityRights,
        new_name: String,
    ) -> Result<SlotId, crate::capability::error::CapabilityError> {
        restrict::<R>(self, from, rights, new_name)
    }

    pub fn revoke(&self, slot: SlotId) -> bool {
        revoke(self, slot, RevokeMode::Single)
    }

    pub fn revoke_tree(&self, root: SlotId) -> usize {
        revoke_tree(self, root)
    }

    pub fn publish_event(&self, ev: CapabilityEvent) {
        let _ = self.inner.events.publish(ev);
    }

    pub(crate) fn next_derived_id(&self) -> crate::core::identity::ids::CapabilityId {
        let raw = self.inner.next_derived.fetch_add(1, Ordering::Relaxed) + 1;
        crate::core::identity::ids::CapabilityId(raw)
    }

    /// Install a derived capability. Phase 5 D2 fix: refuses when
    /// `parent` is no longer in `slots` (Interleaving 2 race).
    pub(crate) fn install_derived<R: Resource>(
        &self,
        parent: SlotId,
        new_cap: Capability<R>,
        new_name: String,
    ) -> Result<SlotId, crate::capability::error::CapabilityError> {
        // Take `parents.write()` FIRST as the serialisation point.
        let mut parents = self.inner.parents.write().expect("cspace poisoned");
        {
            let slots = self.inner.slots.read().expect("cspace poisoned");
            if !slots.contains_key(&parent) {
                return Err(crate::capability::error::CapabilityError::SlotEmpty(parent));
            }
        }
        let new_slot = self.allocate();
        let mut new_cap = new_cap;
        new_cap.bind_slot(new_slot);
        let cap: Arc<dyn crate::capability::handle::cap::AnyCapability> = Arc::new(new_cap);
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

// ---------------------------------------------------------------------------
// Derivation — grant / transfer / restrict
// ---------------------------------------------------------------------------

/// **Grant** — derive a new slot with the given rights; source
/// unchanged. seL4: CNode.Mint.
pub fn grant<R: Resource>(
    space: &CapabilitySpace,
    from: SlotId,
    rights: CapabilityRights,
    new_name: String,
) -> Result<SlotId, crate::capability::error::CapabilityError> {
    derive_with::<R>(space, from, rights, new_name, DeriveKind::Grant)
}

/// **Transfer** — move the capability to a fresh slot. Source cleared.
/// seL4: CNode.Move.
pub fn transfer<R: Resource>(
    space: &CapabilitySpace,
    from: SlotId,
    rights: CapabilityRights,
) -> Result<SlotId, crate::capability::error::CapabilityError> {
    let source_name = space.name_for_slot(from).unwrap_or_default();
    let new_slot = derive_with::<R>(space, from, rights, source_name, DeriveKind::Transfer)?;
    revoke(space, from, RevokeMode::Single);
    Ok(new_slot)
}

/// **Restrict** — derive a strictly less-powerful view of the source.
/// seL4: CNode.Mutate.
pub fn restrict<R: Resource>(
    space: &CapabilitySpace,
    from: SlotId,
    rights: CapabilityRights,
    new_name: String,
) -> Result<SlotId, crate::capability::error::CapabilityError> {
    derive_with::<R>(space, from, rights, new_name, DeriveKind::Restrict)
}

/// Shared body for grant / transfer / restrict. Phase 5 R4/R5:
/// replaces the Phase 4 ~110 LOC of duplicated bodies.
fn derive_with<R: Resource>(
    space: &CapabilitySpace,
    from: SlotId,
    rights: CapabilityRights,
    new_name: String,
    kind: DeriveKind,
) -> Result<SlotId, crate::capability::error::CapabilityError> {
    let source: Arc<Capability<R>> = space
        .lookup_typed::<R>(from)
        .ok_or(crate::capability::error::CapabilityError::SlotEmpty(from))?;
    let held = source.rights();
    if !held.contains(&rights) {
        return Err(
            crate::capability::error::CapabilityError::AttenuationViolation {
                from,
                requested: rights.operations,
                held: held.operations,
            },
        );
    }
    let new_id = space.next_derived_id();
    let derived = source.derive(rights, new_id);
    let new_slot = space.install_derived(from, derived, new_name)?;
    space.publish_event(CapabilityEvent::Derived {
        parent: from,
        child: new_slot,
        kind,
    });
    Ok(new_slot)
}

// ---------------------------------------------------------------------------
// Revocation — revoke (single) and revoke_tree (cascade)
// ---------------------------------------------------------------------------

/// **Revoke** — clear `slot`. Mode-aware:
///
/// - `Single`: clear this slot only. Used by `transfer`.
/// - `Tree`: clear this slot and every descendant.
///
/// Phase 5: both modes share the same lock-acquisition pattern.
/// The kernel-level invariant "every cap has a live parent or is
/// a root" is upheld by the install_derived precondition check
/// (D2); this function relies on that invariant.
pub fn revoke(space: &CapabilitySpace, slot: SlotId, mode: RevokeMode) -> bool {
    if mode == RevokeMode::Single {
        return revoke_single(space, slot);
    }
    revoke_tree(space, slot) >= 1
}

/// Single-slot revoke. Internal helper used by `revoke(Single)`
/// and by `transfer` after the derived slot is installed.
fn revoke_single(space: &CapabilitySpace, slot: SlotId) -> bool {
    let cap_name = space.name_for_slot(slot);

    // Canonical lock order: parents → slots → names.
    let mut parents = space.parents_guarded();
    let mut slots = space.slots_guarded();
    let cap_for_marker: Option<Arc<dyn crate::capability::handle::cap::AnyCapability>> =
        slots.get(&slot).map(|e| e.cap.clone());

    let removed = slots.remove(&slot);
    if removed.is_some() {
        let mut names = space.names_guarded();
        names.retain(|_, s| *s != slot);
        parents.remove(&slot);
        drop(slots);
        drop(names);
        drop(parents);

        if let Some(cap) = cap_for_marker {
            cap.set_revoked_dyn(true);
        }
        space.publish_event(CapabilityEvent::Revoked {
            slot,
            capability: cap_name,
        });
        true
    } else {
        false
    }
}

/// Tree revoke — clear `root` and every descendant. Walks the
/// parent pointer map atomically under the write lock, eliminating
/// the read-then-write gap.
pub fn revoke_tree(space: &CapabilitySpace, root: SlotId) -> usize {
    let mut removed = 0usize;
    let mut visited: std::collections::HashSet<SlotId> = std::collections::HashSet::new();
    let mut frontier = vec![root];
    visited.insert(root);

    while let Some(slot) = frontier.pop() {
        let children: Vec<SlotId> = {
            let parents = space.parents_guarded();
            parents
                .iter()
                .filter_map(|(child, parent)| if *parent == slot { Some(*child) } else { None })
                .collect()
        };
        for c in children {
            if visited.insert(c) {
                frontier.push(c);
            }
        }
        if revoke_single(space, slot) {
            removed += 1;
        }
    }
    space.publish_event(CapabilityEvent::RevokeTree {
        root,
        total: removed,
    });
    removed
}
