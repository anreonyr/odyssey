//! Personality / lifecycle / mint — capability factory + mint-time helpers.
//!
//! Phase 8: this file is the merge of the Phase 5 split
//! (`host/factory.rs` + `host/mint.rs`) into one cohesive
//! personality-side module. The split was justified while
//! `host/` was its own layer; in the new layout the
//! factory + mint helpers live together with the lifecycle
//! actions (mint / ruin / serve).
//!
//! The factory is the only place a capability can be created; this
//! mirrors seL4's `CNode.Allocate`, which the kernel alone performs.
//! The `kind` (sync / stream) is supplied by the caller since the
//! host knows from the manifest whether the capability is streaming.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::core::clock::clock::{Clock, SystemClock};
use crate::core::identity::ids::{CapabilityId, PluginId, SlotId};
use crate::core::identity::kind::CapKind;
use crate::core::meta::meta::CapabilityMeta;
use crate::core::manifest::manifest::CapabilityDecl;
use crate::core::rights::rights::{CapabilityRights, OperationRights};
use crate::capability::handle::cap::Capability;
use crate::capability::enforce::quota::CapabilityBudget;
use crate::core::contract::resource::Resource;
use crate::capability::enforce::space::CapabilitySpace;

// ---------------------------------------------------------------------------
// Mint-time helpers — turn a manifest declaration into a `CapabilityMeta`
// ---------------------------------------------------------------------------

/// Construct a `CapabilityMeta` from a manifest declaration + budget.
/// Used by the factory at mint time.
///
/// Phase 2: namespace defaults to the plugin's FQDN-style name (or the
/// declaration name if the plugin has no namespace). The contract is
/// read straight from `decl.contract_name` — manifests that omit it
/// get an empty contract via `CapabilityContract::default()`.
pub fn meta_from_decl(
    id: CapabilityId,
    decl: &CapabilityDecl,
    plugin: &PluginId,
    budget: &CapabilityBudget,
) -> CapabilityMeta {
    CapabilityMeta {
        id,
        name: decl.name.clone(),
        namespace: namespace_for(plugin, &decl.name),
        // Phase 3 P3.1 — copy the contract name verbatim from the
        // manifest declaration. Empty string means "no contract
        // published" and the resolver will skip this capability
        // when matching `requires[*].contract`.
        contract_name: decl.contract_name.clone(),
        plugin: plugin.clone(),
        in_type: decl.in_type.clone(),
        out_type: decl.out_type.clone(),
        streaming: decl.streaming,
        timeout_ms: budget.timeout_ms(),
        quota: budget.quota_spec(),
    }
}

/// Compute the hierarchical namespace a capability belongs to. For a
/// plugin named `odyssey.model.llama3` exposing `generate`, the
/// namespace is `odyssey.model.llama3.generate`. For a flat name
/// like `counter` under plugin `counter`, it stays `counter`.
pub fn namespace_for(plugin: &PluginId, cap_name: &str) -> String {
    if plugin.name.is_empty() {
        cap_name.to_string()
    } else if cap_name.is_empty() || cap_name == plugin.name {
        plugin.name.clone()
    } else {
        format!("{}.{}", plugin.name, cap_name)
    }
}

// ---------------------------------------------------------------------------
// CapabilityFactory — the mint surface
// ---------------------------------------------------------------------------

/// Mints typed capability tokens and installs them into a
/// `CapabilitySpace`. Cheap to clone; all clones share the same id
/// counter.
///
/// Phase 8 cleanup: drops the redundant `metas: Arc<Mutex<Vec>>`
/// snapshot (which drifted out of sync with `CapabilitySpace` after
/// every revoke — subagent A audit finding) and the dead
/// `snapshots()` / `clock()` accessors (zero production callers).
/// `cspace.enumerate()` is the single source of truth for
/// metadata; the clock is internal to the factory.
#[derive(Clone)]
pub struct CapabilityFactory {
    next_id: Arc<AtomicU64>,
    space: CapabilitySpace,
    clock: Arc<dyn Clock>,
}

impl CapabilityFactory {
    pub fn new(space: CapabilitySpace) -> Self {
        Self::with_clock(space, Arc::new(SystemClock))
    }

    /// Construct a factory that injects `clock` into every
    /// capability it mints. Tests that need a `MockClock` use
    /// this entry point.
    pub fn with_clock(space: CapabilitySpace, clock: Arc<dyn Clock>) -> Self {
        Self {
            next_id: Arc::new(AtomicU64::new(0)),
            space,
            clock,
        }
    }

    /// The space this factory mints into.
    pub fn space(&self) -> &CapabilitySpace {
        &self.space
    }

    /// Mint a typed token wrapping the resource, allocate a slot, install.
    /// Returns the freshly minted `SlotId` so the caller can construct a
    /// typed `Slot<R>` reference to it.
    ///
    /// The factory's `clock` is threaded into the capability so `invoke`
    /// / `invoke_op` can record elapsed wall-clock without calling
    /// `Instant::now()` directly (Phase 5 P1-C fix).
    pub fn mint<R: Resource>(
        &self,
        kind: CapKind,
        decl: &CapabilityDecl,
        plugin: &PluginId,
        budget: CapabilityBudget,
        handler: Arc<R>,
    ) -> SlotId {
        let id = CapabilityId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let meta = meta_from_decl(id, decl, plugin, &budget);
        let rights = CapabilityRights {
            operations: OperationRights::ALL,
            timeout_ms: budget.timeout_ms(),
        };
        let cap = Capability::new(
            meta,
            handler,
            budget,
            rights,
            kind,
            Arc::clone(&self.clock),
        );
        let slot_id = self.space.allocate();
        self.space.install(slot_id, Arc::new(cap));
        slot_id
    }
}
