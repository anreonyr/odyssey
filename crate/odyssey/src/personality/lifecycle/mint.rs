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

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::capability::enforce::quota::CapabilityBudget;
use crate::capability::enforce::space::{CapabilitySpace, PluginCspace};
use crate::capability::handle::cap::Capability;
use crate::core::clock::clock::{Clock, SystemClock};
use crate::core::contract::resource::Resource;
use crate::core::identity::ids::{CapabilityId, PluginId, SlotId};
use crate::core::identity::kind::CapKind;
use crate::core::manifest::manifest::CapabilityDecl;
use crate::core::meta::meta::CapabilityMeta;
use crate::core::rights::rights::{CapabilityRights, OperationRights};

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
        kind: decl.kind,
        timeout_ms: budget.timeout_ms(),
        quota: budget.quota_spec(),
        tool_schema: decl.tool_schema.clone(),
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
///
/// Slice 3 of Direction A: the factory now also tracks per-plugin
/// `PluginCspace`s. Each plugin that wants per-plugin isolation
/// reads its `PluginCspace` via `factory.plugin_cspace(plugin)`,
/// mints into it, and grants a derived slot into the global
/// cspace for cross-plugin / HTTP-bridge visibility. Plugins
/// that haven't migrated yet continue to mint directly into
/// the global cspace via `factory.space()` — the factory
/// still supports that path.
#[derive(Clone)]
pub struct CapabilityFactory {
    next_id: Arc<AtomicU64>,
    space: CapabilitySpace,
    clock: Arc<dyn Clock>,
    plugin_cspaces: Arc<Mutex<HashMap<PluginId, Arc<PluginCspace>>>>,
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
            plugin_cspaces: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// The space this factory mints into. This is the
    /// orchestrator's global cspace — the HTTP bridge and
    /// cross-plugin lookups by name land here. Plugins that
    /// want per-plugin isolation use `plugin_cspace` instead
    /// and grant a derived slot back into this space.
    pub fn space(&self) -> &CapabilitySpace {
        &self.space
    }

    /// The plugin's per-plugin `PluginCspace`. Created on
    /// first access; subsequent calls for the same `PluginId`
    /// return the same instance. Plugins that have migrated
    /// to per-plugin isolation mint their caps into this
    /// cspace and grant a derived slot into the global
    /// cspace; plugins that haven't migrated don't call
    /// this and continue to use `space()`.
    pub fn plugin_cspace(&self, plugin: &PluginId) -> Arc<PluginCspace> {
        let mut pcs = self.plugin_cspaces.lock().expect("plugin_cspaces poisoned");
        pcs.entry(plugin.clone())
            .or_insert_with(|| Arc::new(PluginCspace::new(plugin.clone())))
            .clone()
    }

    /// Drain every slot still live in the plugin's per-plugin
    /// `PluginCspace`. The orchestrator's teardown calls this
    /// for every plugin after its `RuinFn` has run, so a
    /// Slice 3 builtin that mints into its own `PluginCspace`
    /// and grants a derived slot into the global cspace does
    /// not leak the original local slot across restarts.
    ///
    /// Returns the number of slots removed (sum of
    /// `revoke_tree` reports). Returns `0` for a plugin that
    /// never created a `PluginCspace` — i.e. a builtin that
    /// hasn't migrated to per-plugin isolation and therefore
    /// has nothing to leak. Slot ids are resolved from the
    /// cspace's own name table at call time; the reclaim is
    /// idempotent against an already-drained plugin
    /// (`enumerate()` returns `[]`).
    ///
    /// The `RuinFn` signature stays untouched: this is an
    /// orchestrator-side cleanup that the hook does not see.
    /// `default_ruin` continues to revoke only the global
    /// slot ids returned by the matching `MintFn`; the
    /// reclaim happens immediately after, against the
    /// factory's `plugin_cspaces` map.
    pub fn reclaim_plugin(&self, plugin: &PluginId) -> usize {
        let pc = {
            let pcs = self.plugin_cspaces.lock().expect("plugin_cspaces poisoned");
            match pcs.get(plugin) {
                Some(pc) => pc.clone(),
                None => return 0,
            }
        };
        // Snapshot the names under the cspace's locks before
        // we start revoking — `enumerate()` reads the slot
        // table; resolving each name back to a `SlotId` via
        // `slot_for_name` reads the name table. Both read
        // locks; doing the resolve before the revoke loop
        // keeps the iteration independent of any concurrent
        // mutations. A slot id is collected once per name;
        // duplicates (shouldn't happen — cspace names are
        // unique — but the kernel doesn't enforce it) are
        // deduped so the sum is faithful to actual removal.
        let mut roots: Vec<SlotId> = Vec::new();
        for meta in pc.inner().enumerate() {
            if let Some(slot_id) = pc.inner().slot_for_name(&meta.name)
                && !roots.contains(&slot_id) {
                    roots.push(slot_id);
                }
        }
        roots
            .iter()
            .map(|slot_id| pc.inner().revoke_tree(*slot_id))
            .sum()
    }

    /// Mint a typed token wrapping the resource, allocate a slot, install.
    /// Returns the freshly minted `SlotId` so the caller can construct a
    /// typed `Slot<R>` reference to it.
    ///
    /// The factory's `clock` is threaded into the capability so `invoke`
    /// / `invoke_op` can record elapsed wall-clock without calling
    /// `Instant::now()` directly (Phase 5 P1-C fix).
    ///
    /// Slice 3: this path is unchanged. Plugins that want
    /// per-plugin isolation call `plugin_cspace(plugin).mint(...)`
    /// directly (and grant to global themselves); plugins that
    /// haven't migrated continue to call this and end up in
    /// the global cspace. Both paths coexist.
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
        let cap = Capability::new(meta, handler, budget, rights, kind, Arc::clone(&self.clock));
        let slot_id = self.space.allocate();
        self.space.install(slot_id, Arc::new(cap));
        slot_id
    }
}
