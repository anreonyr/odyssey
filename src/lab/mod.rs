//! Phase 1 lab — four repeatable capability experiments.
//!
//! Each `lab::*` module exposes a single `pub fn run() -> Result<...>`
//! that builds its own `CapabilitySpace` + `CapabilityFactory`, mints
//! the Counter and (where relevant) Broker capabilities, and prints a
//! self-explanatory transcript. The six properties the brief lists
//! (authority, delegation, restrict-attenuation, revocation, budget,
//! composition) each get either a dedicated lab or are exercised in
//! the property tests under `tests/capability_lab.rs`.
//!
//! ```text
//! cargo run --bin lab -- authority
//! cargo run --bin lab -- delegation
//! cargo run --bin lab -- revocation
//! cargo run --bin lab -- composition
//! ```
//!
//! The main `odyssey` bin still runs the legacy 7-phase demo so the
//! HTTP bridge and full plugin set keep working.

pub mod agent;
pub mod authority;
pub mod channel;
pub mod composition;
pub mod delegation;
pub mod graph;
pub mod multi_hop;
pub mod namespace;
pub mod quota;
pub mod revocation;

use crate::capability::{
    Capability, CapabilityBudget, CapabilitySpace, CapKind, OperationRights, Slot, SlotId,
};
use crate::host::factory::CapabilityFactory;
use crate::plugins::counter::{counter_plugin, CounterResource};
use serde_json::json;
use std::sync::Arc;

/// Common harness — sets up a `CapabilitySpace`, `CapabilityFactory`,
/// and mints the Counter capability at `slot:counter`. Returns the
/// three things every lab needs.
pub(crate) struct Harness {
    pub cspace: CapabilitySpace,
    pub factory: CapabilityFactory,
    pub counter_slot: SlotId,
}

pub(crate) fn boot_counter() -> Harness {
    let cspace = CapabilitySpace::new();
    let factory = CapabilityFactory::new(cspace.clone());

    // Mint a counter with full authority. (CapabilityMeta itself
    // is constructed by the factory; we don't need to build one here.)
    let decl = crate::host::manifest::CapabilityDecl {
        name: "counter".into(),
        in_type: "object".into(),
        out_type: "object".into(),
        streaming: false,
    };
    let pid = crate::host::manifest::PluginId {
        name: "counter".into(),
        version: "0.1.0".into(),
    };
    let counter_slot = factory.mint::<CounterResource>(
        CapKind::Sync,
        &decl,
        &pid,
        CapabilityBudget::new(5000),
        crate::plugins::counter::handler(),
    );
    Harness {
        cspace,
        factory,
        counter_slot,
    }
}

/// Print a result line with the symbol "✓" for success and "✗" for
/// denial. Phase 1 labs assert outcomes, not raw strings.
pub(crate) fn report(label: &str, ok: bool, detail: impl std::fmt::Display) {
    let mark = if ok { "✓" } else { "✗" };
    println!("  {mark} {label} — {detail}");
    if !ok {
        eprintln!("    [expected denial observed]");
    }
}

/// Assert helper that keeps the labs visually compact:
/// `assert(label, &cap, READ, json!({"op":"read"}))`.
pub(crate) fn assert_authority(
    label: &str,
    cap: &Capability<CounterResource>,
    op: OperationRights,
    input: serde_json::Value,
    expected_ok: bool,
) {
    let res = cap.invoke_op(op, input);
    let ok = res.is_ok() == expected_ok;
    let detail = match res {
        Ok(v) => format!("ok: {v}"),
        Err(e) => format!("err: {e}"),
    };
    report(label, ok, detail);
}

/// Resolve a typed `Slot<CounterResource>` from the harness.
pub(crate) fn counter_slot(h: &Harness) -> Slot<CounterResource> {
    Slot::<CounterResource>::new(h.cspace.clone(), h.counter_slot)
}

/// Compose a fresh Capability<CounterResource> that the caller can use
/// to drive `invoke_op` directly. Most labs do *not* need this because
/// they go through the Slot, but composition wants the typed
/// Capability for the Pipeline.
pub(crate) fn counter_cap(h: &Harness) -> Arc<Capability<CounterResource>> {
    h.cspace
        .lookup_typed::<CounterResource>(h.counter_slot)
        .expect("counter slot must be populated")
}

/// Resolve the metadata name + ops in human-readable form.
pub(crate) fn rights_summary(cap: &Capability<CounterResource>) -> String {
    format!(
        "{:?} timeout={}ms",
        cap.operations(),
        cap.rights().timeout_ms
    )
}

/// Helper used by labs that need the Counter's plugin body for
/// demonstration purposes (the plugin fiber logs, it doesn't run logic).
pub(crate) fn _counter_plugin_for_completeness() -> Arc<dyn cordis::Plugin> {
    counter_plugin()
}

/// Convenience: build a typed `invoke` call to the counter with the
/// given op, returning whether it succeeded.
#[allow(dead_code)]
pub(crate) fn try_counter(
    cap: &Capability<CounterResource>,
    op: OperationRights,
    action: &str,
) -> bool {
    cap.invoke_op(op, json!({ "op": action })).is_ok()
}