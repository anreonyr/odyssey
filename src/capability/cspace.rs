//! `CapabilitySpace` (seL4 CSpace analogue) and the typed `Slot<R>`
//! reference. The four core operations — grant, transfer, restrict,
//! revoke — live on `CapabilitySpace` and have matching convenience
//! methods on `Slot`.

use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

/// True when `namespace` is filed under `prefix` in the hierarchical
/// namespace tree. The empty prefix matches everything; a non-empty
/// prefix matches its own node and every descendant.
fn namespace_prefix_matches(namespace: &str, prefix: &str) -> bool {
    if prefix.is_empty() || prefix == "." {
        return true;
    }
    if namespace == prefix {
        return true;
    }
    namespace.starts_with(&format!("{prefix}."))
}

use serde_json::Value;
use tokio::sync::mpsc;

use super::AnyCapability;
use super::cap::Capability;
use super::resource::Resource;
use super::types::{
    CapabilityChunk, CapabilityError, CapabilityId, CapabilityMeta, CapabilityRights, SlotId,
};

// ---------------------------------------------------------------------------
// Slot<R> — typed reference to a slot (the unit of possession)
// ---------------------------------------------------------------------------

/// Typed, unforgeable reference to a slot.
pub struct Slot<R: Resource> {
    space: super::CapabilitySpace,
    id: SlotId,
    _phantom: PhantomData<R>,
}

impl<R: Resource> Slot<R> {
    pub fn new(space: super::CapabilitySpace, id: SlotId) -> Self {
        Self {
            space,
            id,
            _phantom: PhantomData,
        }
    }

    pub fn id(&self) -> SlotId {
        self.id
    }

    #[allow(dead_code)]
    pub fn space(&self) -> &super::CapabilitySpace {
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
    pub fn kind(&self) -> Option<super::types::CapKind> {
        self.capability().map(|c| c.kind())
    }

    /// Direct sync invocation via the slot.
    pub fn invoke(&self, input: Value) -> Result<Value, String> {
        let cap = self
            .capability()
            .ok_or_else(|| format!("slot {} empty or revoked", self.id.raw()))?;
        cap.invoke(input)
    }

    /// Operation-aware invocation via the slot. Resolves the slot
    /// then defers to `Capability::invoke_op`.
    pub fn invoke_op(
        &self,
        op: super::types::OperationRights,
        input: Value,
    ) -> Result<Value, String> {
        let cap = self
            .capability()
            .ok_or_else(|| format!("slot {} empty or revoked", self.id.raw()))?;
        cap.invoke_op(op, input)
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

    /// **Revoke**: clear the slot this handle points at.
    ///
    /// Note that the typed `Slot<R>` is just a `(CSpace, slot_id)`
    /// pair — it does *not* hold the underlying `Arc<R>`. The
    /// `Arc<R>` lives inside the CSpace at the slot id. So dropping
    /// a `Slot<R>` handle does **not** revoke the cap; the cap is
    /// only released when something calls `cspace.revoke(slot_id)`
    /// (this method is a thin wrapper for that).
    ///
    /// Why it matters: if you hold a producer `Slot<ChannelResource>`
    /// and you want the consumer's stream to terminate, dropping the
    /// producer's `Slot` handle is not enough — the consumer's
    /// `Receiver<CapabilityChunk>` keeps reading because the cap's
    /// `Arc` is still alive inside the cspace. You must call
    /// `cspace.revoke(channel_slot)` (or `Slot::revoke()`) so the
    /// `Arc` count drops to zero and the underlying `mpsc::Sender`
    /// is dropped.
    pub fn revoke(&self) -> bool {
        self.space.revoke(self.id)
    }

    /// **Revoke tree**: clear this slot and every descendant.
    /// Phase 2 P3 — multi-hop revocation propagation. Same Arc
    /// ownership note as [`Self::revoke`].
    pub fn revoke_tree(&self) -> usize {
        self.space.revoke_tree(self.id)
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
// CapabilitySpace — namespace of slots
// ---------------------------------------------------------------------------

/// The capability namespace. Cheap to clone (shares inner state via Arc).
#[derive(Clone)]
pub struct CapabilitySpace {
    inner: Arc<CSpaceInner>,
}

struct CSpaceInner {
    slots: RwLock<HashMap<SlotId, SlotEntry>>,
    names: RwLock<HashMap<String, SlotId>>,
    /// Parent pointer for every derived slot — used by `revoke_tree`
    /// to recursively sever the entire subtree. `None` for roots.
    parents: RwLock<HashMap<SlotId, SlotId>>,
    /// Phase 3 P3.7 — graph event bus. Every mutation in the
    /// cspace (install, revoke, revoke_tree, grant, restrict,
    /// transfer) publishes here. Subscribers see the full
    /// timeline. Defaults to `GraphEventBus::new()`; tests
    /// can pass a smaller-capacity bus via
    /// [`CapabilitySpace::with_bus`].
    events: crate::capability::events::GraphEventBus,
    next: AtomicU64,
    next_derived: AtomicU64,
}

struct SlotEntry {
    /// Single field: Arc<dyn AnyCapability> is both the erased view AND
    /// the typed downcast target (AnyCapability: Any + as_any).
    cap: Arc<dyn AnyCapability>,
}

impl CapabilitySpace {
    pub fn new() -> Self {
        Self::with_bus(crate::capability::events::GraphEventBus::new())
    }

    /// Construct a cspace with a custom event bus. Tests that
    /// want a small-capacity bus to verify overflow handling
    /// use this constructor; production code uses
    /// [`CapabilitySpace::new`].
    pub fn with_bus(bus: crate::capability::events::GraphEventBus) -> Self {
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

    /// Subscribe to the graph event bus. Receives events for
    /// every mutation (install, revoke, revoke_tree, grant,
    /// restrict, transfer) plus boot-level events published
    /// through this bus.
    pub fn subscribe(&self) -> crate::capability::events::GraphEventReceiver {
        self.inner.events.subscribe()
    }

    /// Borrow the event bus directly. Useful for boot to
    /// publish plugin-level events (`PluginActivated`,
    /// `ShutdownStarted`, ...) without going through a cspace
    /// mutation.
    pub fn events(&self) -> &crate::capability::events::GraphEventBus {
        &self.inner.events
    }

    /// Allocate a new slot id. The slot is empty until `install` is called.
    pub fn allocate(&self) -> SlotId {
        let raw = self.inner.next.fetch_add(1, Ordering::Relaxed) + 1;
        SlotId::new(raw)
    }

    /// Install a typed capability into a slot, indexed under the cap's
    /// own `name` in the name index.
    ///
    /// Phase 3 P3.7 — emits `GraphEvent::Minted` on success.
    /// The event carries the cap's `plugin`, the slot id, the
    /// capability name, and the contract name (so subscribers
    /// can log "who minted what").
    pub fn install<R: Resource>(&self, slot: SlotId, cap: Arc<Capability<R>>) {
        let name = cap.name().to_string();
        let contract = cap.meta().contract_name.clone();
        let plugin = cap.meta().plugin.clone();
        let erased: Arc<dyn AnyCapability> = cap;
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
        // Publish outside the locks — broadcast::send is
        // non-blocking but holding the cspace locks across
        // user code is a footgun.
        self.publish_event(crate::capability::events::GraphEvent::Minted {
            plugin,
            slot,
            capability: name,
            contract,
        });
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

    /// Reverse of `lookup_by_name`: given a slot id, return the
    /// name it's currently registered under in the `names` map.
    /// Returns `None` if the slot was revoked or never registered.
    ///
    /// Note: a slot may have a different name in this map than
    /// its `meta().name` — derived caps (from `restrict`/`grant`)
    /// keep the parent's `meta.name` but are registered under a
    /// fresh name. This method returns the **registered** name,
    /// which is what `lookup_by_name` keys against.
    ///
    /// If multiple names point at the same slot (shouldn't happen
    /// in normal use), returns the lexicographically first.
    pub fn name_for_slot(&self, slot: SlotId) -> Option<String> {
        let names = self.inner.names.read().expect("cspace poisoned");
        names
            .iter()
            .filter(|(_, s)| **s == slot)
            .map(|(n, _)| n.clone())
            .min()
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

    /// Phase 2: enumerate capabilities whose namespace starts with
    /// `prefix`. The dot-separated hierarchical match returns every
    /// capability filed under `prefix` and its descendants. Pass an
    /// empty prefix (or `"."`) for the whole space.
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

    /// Phase 2: every distinct direct child of `prefix` in the
    /// namespace tree, with the count of capabilities under it. The
    /// HTTP bridge and the lab graph walker use this to render the
    /// tree.
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

    // -----------------------------------------------------------------------
    // Capability operations: grant / transfer / restrict / revoke
    //
    // seL4 analogue:
    //   grant    — CNode.Mint, parent preserved
    //   transfer — CNode.Move, source cleared
    //   restrict — CNode.Mutate (rights reduction), source preserved
    //   revoke   — CNode.Delete + Revoke
    // -----------------------------------------------------------------------

    /// Mint a fresh `CapabilityId` for a derived capability. ID space
    /// is shared with the factory's root-id counter; the parent→child
    /// relationship is tracked separately in `CSpaceInner.parents` so
    /// the high bit no longer needs to encode lineage.
    fn next_derived_id(&self) -> CapabilityId {
        let raw = self.inner.next_derived.fetch_add(1, Ordering::Relaxed) + 1;
        CapabilityId(raw)
    }

    fn install_derived<R: Resource>(
        &self,
        parent: SlotId,
        new_cap: Capability<R>,
        new_name: String,
    ) -> SlotId {
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
        drop(names);
        drop(slots);
        // Record parent so `revoke_tree` can sever the subtree.
        self.inner
            .parents
            .write()
            .expect("cspace poisoned")
            .insert(new_slot, parent);
        new_slot
    }

    /// Phase 3 P3.7 — publish a `Derived` event for a freshly
    /// installed derived slot. Called from `grant`, `restrict`,
    /// and `transfer` after `install_derived` registers the
    /// slot. The `kind` distinguishes the three derivation
    /// paths so subscribers can audit authority flow.
    fn publish_derived(
        &self,
        parent: SlotId,
        child: SlotId,
        kind: crate::capability::events::DeriveKind,
    ) {
        self.publish_event(crate::capability::events::GraphEvent::Derived {
            parent,
            child,
            kind,
        });
    }

    /// **Grant**: derive a new slot with the given rights; source slot
    /// is unchanged. New slot is registered under `new_name`.
    ///
    /// Attenuation invariant: the requested rights must be a subset of
    /// the source's rights — both the operation bits and the timeout
    /// ceiling. You cannot amplify authority you don't have.
    pub fn grant<R: Resource>(
        &self,
        from: SlotId,
        rights: CapabilityRights,
        new_name: String,
    ) -> Result<SlotId, CapabilityError> {
        let source: Arc<Capability<R>> = self
            .lookup_typed::<R>(from)
            .ok_or(CapabilityError::SlotEmpty(from))?;
        let held = source.rights();
        if !held.contains(&rights) {
            return Err(CapabilityError::AttenuationViolation {
                from,
                requested: rights.operations,
                held: held.operations,
            });
        }
        let new_id = self.next_derived_id();
        let derived = source.derive(rights, new_id);
        let new_slot = self.install_derived(from, derived, new_name);
        // Phase 3 P3.7 — Derived event for grant.
        self.publish_derived(
            from,
            new_slot,
            crate::capability::events::DeriveKind::Grant,
        );
        Ok(new_slot)
    }

    /// **Transfer**: move the capability to a fresh slot with the given
    /// rights. New slot takes the source's name. Source slot is cleared.
    /// Attenuation is required — you can only transfer authority you hold.
    pub fn transfer<R: Resource>(
        &self,
        from: SlotId,
        rights: CapabilityRights,
    ) -> Result<SlotId, CapabilityError> {
        let source: Arc<Capability<R>> = self
            .lookup_typed::<R>(from)
            .ok_or(CapabilityError::SlotEmpty(from))?;
        let held = source.rights();
        if !held.contains(&rights) {
            return Err(CapabilityError::AttenuationViolation {
                from,
                requested: rights.operations,
                held: held.operations,
            });
        }
        let source_name = source.name().to_string();
        let new_id = self.next_derived_id();
        let derived = source.derive(rights, new_id);
        let new_slot = self.install_derived(from, derived, source_name);
        // Phase 3 P3.7 — Derived event for transfer.
        self.publish_derived(
            from,
            new_slot,
            crate::capability::events::DeriveKind::Transfer,
        );
        self.revoke(from);
        Ok(new_slot)
    }

    /// **Restrict**: derive a new slot with rights that must be a strict
    /// subset (or equal) of the parent's. Mechanically identical to
    /// `grant`; provided as a distinct method so the call site
    /// documents intent — "I am dropping authority" vs. "I am
    /// propagating a peer view".
    ///
    /// Both `grant` and `restrict` enforce `rights(child) ⊆
    /// rights(parent)`; the difference is the seL4-style mental model:
    /// grant = minting a peer's view, restrict = dropping privileges on
    /// yourself.
    ///
    /// Phase 3 P3.7 — `restrict` emits a `Derived` event with
    /// `kind = Restrict` (not `Grant`) so subscribers can
    /// distinguish the two flows for audit. We don't delegate
    /// to `grant` here because the Derived event must carry
    /// the right kind.
    pub fn restrict<R: Resource>(
        &self,
        from: SlotId,
        rights: CapabilityRights,
        new_name: String,
    ) -> Result<SlotId, CapabilityError> {
        let source: Arc<Capability<R>> = self
            .lookup_typed::<R>(from)
            .ok_or(CapabilityError::SlotEmpty(from))?;
        let held = source.rights();
        if !held.contains(&rights) {
            return Err(CapabilityError::AttenuationViolation {
                from,
                requested: rights.operations,
                held: held.operations,
            });
        }
        let new_id = self.next_derived_id();
        let derived = source.derive(rights, new_id);
        let new_slot = self.install_derived(from, derived, new_name);
        self.publish_derived(
            from,
            new_slot,
            crate::capability::events::DeriveKind::Restrict,
        );
        Ok(new_slot)
    }

    /// **Revoke**: clear a slot. The slot id remains valid (stale
    /// `Slot<R>` references don't panic), but `capability()` /
    /// `invoke()` / `open()` will fail.
    ///
    /// Phase 3 P3.7 — emits `GraphEvent::Revoked` on success.
    /// The event carries the cap's registered name (or
    /// `None` if it wasn't in the names map) so subscribers
    /// can log "what died" without re-reading the cspace.
    pub fn revoke(&self, slot: SlotId) -> bool {
        // Phase 3 P3.7 — capture the registered name before
        // clearing, so the Revoked event can carry it.
        let cap_name: Option<String> = self
            .inner
            .names
            .read()
            .expect("cspace poisoned")
            .iter()
            .find_map(|(n, s)| if *s == slot { Some(n.clone()) } else { None });

        let mut slots = self.inner.slots.write().expect("cspace poisoned");
        let removed = slots.remove(&slot);
        if removed.is_some() {
            let mut names = self.inner.names.write().expect("cspace poisoned");
            names.retain(|_, s| *s != slot);
            let mut parents = self.inner.parents.write().expect("cspace poisoned");
            parents.remove(&slot);
            drop(slots);
            drop(names);
            drop(parents);
            self.publish_event(crate::capability::events::GraphEvent::Revoked {
                slot,
                capability: cap_name,
            });
            true
        } else {
            false
        }
    }

    /// **Revoke tree** (Phase 2 P3): revoke a slot and every
    /// descendant slot derived from it. Returns the count of slots
    /// removed. This makes revocation propagate down multi-hop
    /// delegation chains (Broker A → Broker B → ...) so that an
    /// intermediate revocation severs every downstream authority.
    ///
    /// Phase 3 P3.7 — emits one `Revoked` event per slot that
    /// was cleared (via the inner `revoke` call) plus one
    /// `RevokeTree` event with the total at the end. The
    /// `RevokeTree` event marks the operation as a whole so
    /// subscribers can distinguish "single revoke" from
    /// "subtree wipe".
    pub fn revoke_tree(&self, root: SlotId) -> usize {
        let mut removed = 0usize;
        let mut frontier = vec![root];
        while let Some(slot) = frontier.pop() {
            // Find children (slots whose parent == slot).
            let children: Vec<SlotId> = {
                let parents = self.inner.parents.read().expect("cspace poisoned");
                parents
                    .iter()
                    .filter_map(|(child, parent)| if *parent == slot { Some(*child) } else { None })
                    .collect()
            };
            // Recurse into children first so we don't drop the parent
            // pointer while still walking the tree.
            for c in children {
                frontier.push(c);
            }
            // Now revoke this slot. Each successful revoke
            // publishes its own Revoked event (P3.7).
            if self.revoke(slot) {
                removed += 1;
            }
        }
        self.publish_event(crate::capability::events::GraphEvent::RevokeTree { root, total: removed });
        removed
    }

    /// Total number of populated slots — useful for graph assertions.
    pub fn len(&self) -> usize {
        self.inner.slots.read().expect("cspace poisoned").len()
    }

    /// True when no slots are populated. Complements [`len`]
    /// so callers can use the idiomatic `is_empty()` check
    /// instead of `len() == 0`.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Publish a graph event with diagnostics.
    ///
    /// Fast path: when no subscribers are connected (the common
    /// case in unit tests that don't exercise the event bus),
    /// the event is silently dropped. No log spam, no allocation.
    ///
    /// Subscribers present but buffer overflowed: print a one-line
    /// warning to stderr. Subscribers can't keep up means the
    /// audit trail is being lost — better to surface that than
    /// silently swallow it.
    ///
    /// This helper exists so callers don't need to repeat the
    /// `let _ = self.inner.events.publish(...)` pattern; it's
    /// also the single place to add fancier behaviour later
    /// (e.g. structured logging, metrics).
    pub fn publish_event(&self, ev: crate::capability::events::GraphEvent) {
        if self.inner.events.receiver_count() == 0 {
            return;
        }
        if let Err(_dropped) = self.inner.events.publish(ev) {
            eprintln!(
                "[cspace] graph event dropped: subscribers lagging (broadcast buffer overflow)"
            );
        }
    }

    /// Phase 2 P6: every slot whose recorded parent is `parent`.
    pub fn children_of(&self, parent: SlotId) -> Vec<SlotId> {
        self.inner
            .parents
            .read()
            .expect("cspace poisoned")
            .iter()
            .filter_map(|(child, p)| if *p == parent { Some(*child) } else { None })
            .collect()
    }
}

impl Default for CapabilitySpace {
    fn default() -> Self {
        Self::new()
    }
}

