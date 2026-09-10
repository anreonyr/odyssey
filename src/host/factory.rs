//! Capability factory — mints typed `Capability<R>` and installs them
//! into a `CapabilitySpace`.
//!
//! The factory is the only place a capability can be created; this mirrors
//! seL4's `CNode.Allocate`, which the kernel alone performs. The `kind`
//! (sync / stream) is supplied by the caller since the host knows from
//! the manifest whether the capability is streaming.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::host::manifest::CapabilityDecl;
use crate::kernel::ids::PluginId;
use crate::host::mint::meta_from_decl;
use crate::kernel::clock::SystemClock;
use crate::kernel::ids::CapabilityId;
use crate::kernel::quota::CapabilityBudget;
use crate::kernel::{
    Capability, CapabilityMeta, CapabilitySpace, CapKind, Resource,
};

/// Mints typed capability tokens and installs them into a
/// `CapabilitySpace`. Cheap to clone; all clones share the same id
/// counter and meta registry.
#[derive(Clone)]
pub struct CapabilityFactory {
    next_id: Arc<AtomicU64>,
    metas: Arc<Mutex<Vec<CapabilityMeta>>>,
    space: CapabilitySpace,
    /// Wall-clock source threaded into every minted
    /// capability. The factory defaults to `SystemClock`;
    /// tests that need deterministic time should construct
    /// their own factory via `with_clock` (or pass the mock
    /// clock into `mint_with_clock` directly).
    clock: Arc<dyn crate::kernel::clock::Clock>,
}

impl CapabilityFactory {
    pub fn new(space: CapabilitySpace) -> Self {
        Self::with_clock(space, Arc::new(SystemClock))
    }

    /// Construct a factory that injects `clock` into every
    /// capability it mints. Tests that need a `MockClock` use
    /// this entry point.
    pub fn with_clock(
        space: CapabilitySpace,
        clock: Arc<dyn crate::kernel::clock::Clock>,
    ) -> Self {
        Self {
            next_id: Arc::new(AtomicU64::new(0)),
            metas: Arc::new(Mutex::new(Vec::new())),
            space,
            clock,
        }
    }

    /// The space this factory mints into.
    pub fn space(&self) -> &CapabilitySpace {
        &self.space
    }

    /// The clock this factory injects into every minted capability.
    pub fn clock(&self) -> &Arc<dyn crate::kernel::clock::Clock> {
        &self.clock
    }

    /// Mint a typed token wrapping the resource, allocate a slot, install.
    /// The `kind` is supplied by the caller (typically from the
    /// manifest's `streaming` flag). The factory's `clock` is
    /// threaded into the capability so `invoke` / `invoke_op` can
    /// record elapsed wall-clock without calling `Instant::now()`
    /// directly (Phase 5 P1-C fix).
    pub fn mint<R: Resource>(
        &self,
        kind: CapKind,
        decl: &CapabilityDecl,
        plugin: &PluginId,
        budget: CapabilityBudget,
        handler: Arc<R>,
    ) -> crate::kernel::ids::SlotId {
        let id = CapabilityId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let meta = meta_from_decl(id, decl, plugin, &budget);
        let rights = crate::kernel::rights::CapabilityRights {
            operations: crate::kernel::rights::OperationRights::ALL,
            timeout_ms: budget.timeout_ms(),
        };
        let cap = Capability::new(
            meta.clone(),
            handler,
            budget,
            rights,
            kind,
            Arc::clone(&self.clock),
        );
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