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

use super::cap::Capability;
use super::resource::Resource;
use super::types::{
    CapabilityChunk, CapabilityError, CapabilityId, CapabilityMeta, CapabilityRights, SlotId,
};
use super::AnyCapability;

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
        Self { space, id, _phantom: PhantomData }
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

    /// **Revoke**: clear this slot.
    pub fn revoke(&self) -> bool {
        self.space.revoke(self.id)
    }

    /// **Revoke tree**: clear this slot and every descendant.
    /// Phase 2 P3 — multi-hop revocation propagation.
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
        Self {
            inner: Arc::new(CSpaceInner {
                slots: RwLock::new(HashMap::new()),
                names: RwLock::new(HashMap::new()),
                parents: RwLock::new(HashMap::new()),
                next: AtomicU64::new(0),
                next_derived: AtomicU64::new(0),
            }),
        }
    }

    /// Allocate a new slot id. The slot is empty until `install` is called.
    pub fn allocate(&self) -> SlotId {
        let raw = self.inner.next.fetch_add(1, Ordering::Relaxed) + 1;
        SlotId::new(raw)
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
        let mut out: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
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
        Ok(self.install_derived(from, derived, new_name))
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
            let mut parents = self.inner.parents.write().expect("cspace poisoned");
            parents.remove(&slot);
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
            // Now revoke this slot.
            if self.revoke(slot) {
                removed += 1;
            }
        }
        removed
    }

    /// Total number of populated slots — useful for graph assertions.
    pub fn len(&self) -> usize {
        self.inner.slots.read().expect("cspace poisoned").len()
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