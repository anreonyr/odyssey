//! Capability factory — mints typed `Capability<R>` and installs them
//! into a `CapabilitySpace`.
//!
//! The factory is the only place a capability can be created; this mirrors
//! seL4's `CNode.Allocate`, which the kernel alone performs. The `kind`
//! (sync / stream) is supplied by the caller since the host knows from
//! the manifest whether the capability is streaming.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::capability::{
    meta_from_decl, Capability, CapabilityBudget, CapabilityId, CapabilityMeta, CapabilitySpace,
    CapKind, Resource,
};
use crate::host::manifest::{CapabilityDecl, PluginId};

/// Mints typed capability tokens and installs them into a
/// `CapabilitySpace`. Cheap to clone; all clones share the same id
/// counter and meta registry.
#[derive(Clone)]
pub struct CapabilityFactory {
    next_id: Arc<AtomicU64>,
    metas: Arc<Mutex<Vec<CapabilityMeta>>>,
    space: CapabilitySpace,
}

impl CapabilityFactory {
    pub fn new(space: CapabilitySpace) -> Self {
        Self {
            next_id: Arc::new(AtomicU64::new(0)),
            metas: Arc::new(Mutex::new(Vec::new())),
            space,
        }
    }

    /// The space this factory mints into.
    pub fn space(&self) -> &CapabilitySpace {
        &self.space
    }

    /// Mint a typed token wrapping the resource, allocate a slot, install.
    /// The `kind` is supplied by the caller (typically from the
    /// manifest's `streaming` flag).
    pub fn mint<R: Resource>(
        &self,
        kind: CapKind,
        decl: &CapabilityDecl,
        plugin: &PluginId,
        budget: CapabilityBudget,
        handler: Arc<R>,
    ) -> crate::capability::SlotId {
        let id = CapabilityId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let budget = Arc::new(budget);
        let meta = meta_from_decl(id, decl, plugin, &budget);
        let cap = Capability::new(meta.clone(), handler, budget, kind);
        let slot = self.space.allocate();
        self.space.install(slot, Arc::new(cap));
        self.metas.lock().expect("factory poisoned").push(meta);
        slot
    }

    /// Snapshot of every minted token's metadata, sorted by name.
    pub fn snapshots(&self) -> Vec<CapabilityMeta> {
        let mut out: Vec<_> = self.metas.lock().expect("factory poisoned").clone();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }
}