//! RuleAgent — type-agnostic orchestrator that dispatches via the
//! capability contract, not via hardcoded resource types.
//!
//! ## Phase 3.4 (capability-native)
//!
//! The agent's *reachable set* — the set of capability handles
//! it can address — is derived from the **resolved binding
//! table**, not from a hardcoded slot list. Concretely:
//!
//! - The agent's `manifest.toml` declares `[[requires]]` entries
//!   listing which capabilities it depends on.
//! - The resolver (`host::resolver`) walks the dependency
//!   graph at boot and produces `ResolvedPlan::bindings`:
//!   `{ handle, provider, capability, contract }` for each
//!   consumer.
//! - `AgentResource::from_bindings` reads
//!   `plan.bindings[consumer_plugin_id]` and stores one
//!   `Reachable { handle, capability }` per binding. The
//!   `handle` is the local name (what the agent uses to
//!   dispatch); the `capability` is the cspace name (what
//!   `lookup_by_name` resolves against).
//! - At dispatch, the agent matches the input's `target` against
//!   reachable handles. If matched, it looks up the cap via
//!   `cspace.lookup_by_name(reachable.capability)`.
//!
//! ## Authority check (carried over from P5)
//!
//! Once a cap is reachable, the agent still inspects the
//! contract to translate the action verb into the right
//! `OperationRights` bit, and the kernel-level guard inside
//! `invoke_op_dyn` enforces `requested ⊆ held`. The agent does
//! not invent mappings — the cap's contract is the authoritative
//! vocabulary.
//!
//! ## File layout (Phase 6 split)
//!
//! The agent module was a single 712-LOC `handler.rs` file; the
//! Phase 6 split broke it into focused pieces:
//!
//! - `handler.rs`  (this file) — struct, constructors,
//!   accessors, the `Reachable` re-export, plus the shared
//!   `parse_operation` helper used by both the sync and the
//!   streaming dispatch paths.
//! - `dispatch.rs` — `impl Resource for AgentResource { fn
//!   invoke }`. Sync dispatch logic with reachable + cspace +
//!   authority checks.
//! - `stream.rs`   — `impl Resource for AgentResource { fn open
//!   }`, the `run_program` interpreter, `RunStats`, and the
//!   per-event emit helpers.
//! - `plugin.rs`   — cordis integration: `handler`,
//!   `handler_from_plan`, `agent_plugin`.
//! - `panic.rs`    — `panic_payload_to_str` formatter used by
//!   the streaming path's panic-catch.

use crate::host::resolver::{ResolvedBinding, ResolvedPlan};
use crate::kernel::ids::PluginId;
use crate::kernel::{CapabilitySpace, OperationRights};

/// Parse a contract-published operation string ("READ", "WRITE",
/// "EXECUTE", "ADMIN") into the corresponding bit. Returns `None`
/// for any other value — the cap author is responsible for
/// publishing only valid strings in the contract.
///
/// Shared by the sync dispatch path (`dispatch.rs`) and the
/// streaming path (`stream.rs`); the streaming path uses it
/// twice per step (once for authority, once for the dispatch
/// arm).
pub(crate) fn parse_operation(s: &str) -> Option<OperationRights> {
    // Direct match first (Phase 1-3 contract style).
    if let Some(op) = match s {
        "READ" => Some(OperationRights::READ),
        "WRITE" => Some(OperationRights::WRITE),
        "EXECUTE" => Some(OperationRights::EXECUTE),
        "ADMIN" => Some(OperationRights::ADMIN),
        _ => None,
    } {
        return Some(op);
    }
    // Phase 4 P4.6 — many contracts use a plugin-prefixed
    // style ("DB_WRITE", "HTTP_REQUEST", "GENERATE"). The
    // prefix is whatever comes before the final `_`. Strip
    // it and re-match on the suffix.
    if let Some((_, suffix)) = s.rsplit_once('_') {
        match suffix {
            "READ" => Some(OperationRights::READ),
            "WRITE" => Some(OperationRights::WRITE),
            "EXECUTE" => Some(OperationRights::EXECUTE),
            "ADMIN" => Some(OperationRights::ADMIN),
            _ => None,
        }
    } else {
        None
    }
}

/// Reachable moved out of `crate::capability` (where it lived
/// in Phase 4 P4.1) into `host::resolver::plan` (Phase 5 R5).
/// The agent test surface still imports it from here for
/// ergonomic reach.
pub use crate::host::resolver::Reachable;

/// Holds the agent's identity, the reachable set derived from
/// its binding table, and a reference to the cspace so it can
/// resolve them. Built via [`AgentResource::from_bindings`].
///
/// Note: no `SlotId`s are stored. The agent never sees slot
/// identifiers — only the binding table's `handle` and
/// `capability` strings.
pub struct AgentResource {
    name: String,
    /// Reachable entries from the binding table, sorted by
    /// `handle` for stable error messages and stable dispatch
    /// when the input has multiple candidates (it never should
    /// — duplicate handles are an error at construction).
    reachable: Vec<Reachable>,
    cspace: CapabilitySpace,
}

impl AgentResource {
    /// Construct from a [`ResolvedPlan`]. The reachable set is
    /// the set of bindings for `consumer` in `plan.bindings`.
    /// If `consumer` has no bindings (no `[[requires]]`),
    /// reachable is empty and the agent rejects every dispatch.
    pub fn from_bindings(
        name: impl Into<String>,
        plan: &ResolvedPlan,
        consumer: &PluginId,
        cspace: CapabilitySpace,
    ) -> Self {
        let mut reachable: Vec<Reachable> = plan
            .bindings
            .get(consumer)
            .map(|bs| bs.iter().map(Reachable::from_binding).collect())
            .unwrap_or_default();
        reachable.sort_by(|a, b| a.handle.cmp(&b.handle));
        // C2: log the reachable set at construction so a
        // missing-binding situation (empty `[[requires]]` ⇒
        // empty reachable) is visible immediately. Empty
        // reachable means "every dispatch will fail" — should
        // be loud, not silent until first invoke.
        let name_string = name.into();
        eprintln!(
            "[agent] {n}: reachable=[{list}]",
            n = name_string,
            list = reachable
                .iter()
                .map(|r| format!("{}→{}", r.handle, r.capability))
                .collect::<Vec<_>>()
                .join(", ")
        );
        // Duplicate handles mean the resolver produced two
        // bindings with the same `name` for the same consumer.
        // That's a resolver bug — surface it loudly here rather
        // than silently picking one.
        debug_assert!(
            reachable.windows(2).all(|w| w[0].handle != w[1].handle),
            "duplicate handles in binding table for consumer {:?}: {:?}",
            consumer,
            reachable.iter().map(|r| &r.handle).collect::<Vec<_>>()
        );
        // Strip duplicates only in release builds so tests can
        // still construct the resource after the assert fires.
        reachable.dedup_by(|a, b| a.handle == b.handle);
        Self {
            name: name_string,
            reachable,
            cspace,
        }
    }

    /// Phase 4 P4.6 — construct directly from a hand-rolled
    /// reachable set. Used by tests that build agents whose
    /// reachable references derived (restricted) capabilities
    /// minted outside the resolver's view.
    ///
    /// Production code should use [`AgentResource::from_bindings`]
    /// so the resolver drives the binding shape. This entry
    /// point exists for delegation tests.
    pub fn from_reachable(
        name: impl Into<String>,
        reachable: Vec<Reachable>,
        cspace: CapabilitySpace,
    ) -> Self {
        let name_string = name.into();
        let mut sorted = reachable;
        sorted.sort_by(|a, b| a.handle.cmp(&b.handle));
        eprintln!(
            "[agent] {n}: reachable=[{list}]",
            n = name_string,
            list = sorted
                .iter()
                .map(|r| format!("{}→{}", r.handle, r.capability))
                .collect::<Vec<_>>()
                .join(", ")
        );
        Self {
            name: name_string,
            reachable: sorted,
            cspace,
        }
    }

    /// Inspect the reachable set. Tests assert the binding table
    /// shape without going through dispatch.
    pub fn reachable(&self) -> &[Reachable] {
        &self.reachable
    }

    /// Borrow the agent's name. Used by `dispatch.rs` and
    /// `stream.rs` to format error messages and event payloads.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// Clone the reachable set. `stream.rs::run_program`
    /// consumes the clone into the `tokio::spawn` boundary so
    /// the spawned task does not borrow from `self`.
    pub(crate) fn reachable_vec(&self) -> Vec<Reachable> {
        self.reachable.clone()
    }

    /// Borrow the cspace. `dispatch.rs` uses this for the sync
    /// path's `lookup_by_name`; `stream.rs` clones it into the
    /// spawned task.
    pub(crate) fn cspace(&self) -> &CapabilitySpace {
        &self.cspace
    }

    /// Look up a specific binding by handle. Returns the
    /// `ResolvedBinding` (handle, provider, capability, contract)
    /// if the handle is reachable, or `None` otherwise.
    pub fn binding_for<'a>(
        plan: &'a ResolvedPlan,
        consumer: &PluginId,
        handle: &str,
    ) -> Option<&'a ResolvedBinding> {
        plan.bindings
            .get(consumer)?
            .iter()
            .find(|b| b.handle == handle)
    }
}

// `Resource` impl lives in `dispatch.rs` (sync `invoke`) and
// `stream.rs` (async `open`). The struct itself stays here so
// the constructors and accessors are discoverable in one place.
