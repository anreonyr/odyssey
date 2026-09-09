//! Capability factory — mints typed `Capability<R, K>` tokens.
//!
//! The factory is the only place a capability can be created; this mirrors
//! seL4's `CNode.Allocate`, which the kernel alone performs. The generic
//! `R` parameter locks the minted token to the resource type the plugin
//! module declared; the generic `K` parameter locks the sync vs stream kind.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::capability::{
    meta_from_decl, Capability, CapabilityBudget, CapabilityId, CapabilityMeta, CapabilityKind,
    StreamResource, SyncKind, SyncResource,
};
use crate::manifest::{CapabilityDecl, PluginId};

/// Mints typed capability tokens. Cheap to clone; all clones share the
/// same id counter and meta registry.
#[derive(Clone, Default)]
pub struct CapabilityFactory {
    next_id: Arc<AtomicU64>,
    metas: Arc<Mutex<Vec<CapabilityMeta>>>,
}

impl CapabilityFactory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mint a sync token wrapping a `SyncResource` of type `R`.
    pub fn mint_sync<R: SyncResource + 'static>(
        &self,
        decl: &CapabilityDecl,
        plugin: &PluginId,
        budget: CapabilityBudget,
        handler: Arc<R>,
    ) -> Arc<Capability<R, SyncKind>> {
        self.mint_typed::<R, SyncKind>(decl, plugin, budget, handler)
    }

    /// Mint a streaming token wrapping a `StreamResource` of type `R`.
    pub fn mint_stream<R: StreamResource + 'static>(
        &self,
        decl: &CapabilityDecl,
        plugin: &PluginId,
        budget: CapabilityBudget,
        handler: Arc<R>,
    ) -> Arc<Capability<R, crate::capability::StreamKind>> {
        self.mint_typed::<R, crate::capability::StreamKind>(decl, plugin, budget, handler)
    }

    fn mint_typed<R, K>(
        &self,
        decl: &CapabilityDecl,
        plugin: &PluginId,
        budget: CapabilityBudget,
        handler: Arc<R>,
    ) -> Arc<Capability<R, K>>
    where
        R: Send + Sync + 'static,
        K: CapabilityKind,
    {
        let id = CapabilityId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let budget = Arc::new(budget);
        let meta = meta_from_decl(id, decl, plugin, &budget);
        let token = Arc::new(Capability::new_typed(meta, handler, budget));
        self.metas
            .lock()
            .expect("factory poisoned")
            .push(token.meta().clone());
        token
    }

    /// Snapshot of every minted token's metadata, sorted by name. Useful
    /// for diagnostics and the HTTP bridge.
    pub fn snapshots(&self) -> Vec<CapabilityMeta> {
        let mut out: Vec<_> = self.metas.lock().expect("factory poisoned").clone();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }
}

// Capability constructor used by the factory's mint_typed (avoids exposing
// both new_sync and new_stream generically while still allowing the kind
// Capability<R, K>::new_typed lives in capability.rs as pub(crate).