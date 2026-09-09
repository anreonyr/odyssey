//! Capability factory — mints typed `Capability<R, K>` and installs them
//! into a `CapabilitySpace` at a freshly allocated slot.
//!
//! seL4 mapping: `CNode.Allocate` (the kernel alone creates slots).
//! Here: only the factory creates and installs.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::capability::{
    meta_from_decl, Capability, CapabilityBudget, CapabilityId, CapabilityMeta, CapabilitySpace,
    StreamResource, SyncKind, SyncResource,
};
use crate::manifest::{CapabilityDecl, PluginId};

/// Mints typed capability tokens and installs them into a `CapabilitySpace`.
/// Cheap to clone; all clones share the same id counter and meta registry.
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

    /// Mint a sync token, allocate a slot in the CSpace, install the cap,
    /// and return the slot id.
    pub fn mint_sync<R: SyncResource + 'static>(
        &self,
        decl: &CapabilityDecl,
        plugin: &PluginId,
        budget: CapabilityBudget,
        handler: Arc<R>,
    ) -> crate::capability::SlotId {
        self.mint_typed_sync::<R>(decl, plugin, budget, handler)
    }

    /// Mint a streaming token, allocate a slot, install, return slot id.
    pub fn mint_stream<R: StreamResource + 'static>(
        &self,
        decl: &CapabilityDecl,
        plugin: &PluginId,
        budget: CapabilityBudget,
        handler: Arc<R>,
    ) -> crate::capability::SlotId {
        self.mint_typed_stream::<R>(decl, plugin, budget, handler)
    }

    fn mint_typed_sync<R>(
        &self,
        decl: &CapabilityDecl,
        plugin: &PluginId,
        budget: CapabilityBudget,
        handler: Arc<R>,
    ) -> crate::capability::SlotId
    where
        R: SyncResource + 'static,
    {
        let id = CapabilityId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let budget = Arc::new(budget);
        let meta = meta_from_decl(id, decl, plugin, &budget);
        let cap: Arc<Capability<R, SyncKind>> =
            Arc::new(Capability::new_typed(meta.clone(), handler, budget));
        let erased: Arc<dyn crate::capability::AnyCapability> = cap.clone();
        let typed: Arc<dyn std::any::Any + Send + Sync> = cap;
        let slot = self.space.allocate();
        self.space.install(slot, erased, typed);
        self.metas.lock().expect("factory poisoned").push(meta);
        slot
    }

    fn mint_typed_stream<R>(
        &self,
        decl: &CapabilityDecl,
        plugin: &PluginId,
        budget: CapabilityBudget,
        handler: Arc<R>,
    ) -> crate::capability::SlotId
    where
        R: StreamResource + 'static,
    {
        let id = CapabilityId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let budget = Arc::new(budget);
        let meta = meta_from_decl(id, decl, plugin, &budget);
        let cap: Arc<Capability<R, crate::capability::StreamKind>> =
            Arc::new(Capability::new_typed(meta.clone(), handler, budget));
        let erased: Arc<dyn crate::capability::AnyCapability> = cap.clone();
        let typed: Arc<dyn std::any::Any + Send + Sync> = cap;
        let slot = self.space.allocate();
        self.space.install(slot, erased, typed);
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