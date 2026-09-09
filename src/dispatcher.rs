//! Capability factory — mints and tracks tokens.
//!
//! The factory is the only place a [`CapabilityToken`] can be created; this
//! mirrors seL4's `CNode.Allocate`, which the kernel alone performs. Plugins
//! receive minted tokens via cordis inject and never touch the factory
//! directly.
//!
//! Inner state is wrapped in `Arc`, so `Clone` is cheap and shares. Provided
//! as the `capability_factory` cordis service.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::capability::{
    CapabilityBudget, CapabilityId, CapabilityMeta, CapabilityToken, StreamInvoke, SyncInvoke,
};
use crate::manifest::{CapabilityDecl, PluginId};

/// Mints capability tokens. Cheap to clone; all clones share the same id
/// counter and token registry.
#[derive(Clone, Default)]
pub struct CapabilityFactory {
    next_id: Arc<AtomicU64>,
    tokens: Arc<Mutex<Vec<Arc<CapabilityToken>>>>,
}

impl CapabilityFactory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mint a sync token (seL4: CNode.Allocate analogue).
    pub fn mint_sync(
        &self,
        decl: &CapabilityDecl,
        plugin: &PluginId,
        budget: CapabilityBudget,
        handler: Arc<dyn SyncInvoke>,
    ) -> Arc<CapabilityToken> {
        let id = CapabilityId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let budget = Arc::new(budget);
        let token = Arc::new(CapabilityToken::new_sync(
            decl,
            plugin,
            id,
            budget,
            handler,
        ));
        self.tokens.lock().expect("factory poisoned").push(token.clone());
        token
    }

    /// Mint a streaming token.
    pub fn mint_stream(
        &self,
        decl: &CapabilityDecl,
        plugin: &PluginId,
        budget: CapabilityBudget,
        handler: Arc<dyn StreamInvoke>,
    ) -> Arc<CapabilityToken> {
        let id = CapabilityId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let budget = Arc::new(budget);
        let token = Arc::new(CapabilityToken::new_stream(
            decl,
            plugin,
            id,
            budget,
            handler,
        ));
        self.tokens.lock().expect("factory poisoned").push(token.clone());
        token
    }

    /// Snapshot of every minted token's metadata, sorted by name. Useful
    /// for diagnostics and the HTTP bridge.
    pub fn snapshots(&self) -> Vec<CapabilityMeta> {
        let tokens = self.tokens.lock().expect("factory poisoned");
        let mut metas: Vec<_> = tokens.iter().map(|t| t.meta().clone()).collect();
        metas.sort_by(|a, b| a.name.cmp(&b.name));
        metas
    }
}