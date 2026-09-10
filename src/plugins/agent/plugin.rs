//! Cordis plugin integration for the agent.
//!
//! Phase 6 split: this file owns the cordis glue — the legacy
//! slot-id constructor (`handler`), the capability-native
//! constructor (`handler_from_plan`), and the plugin factory
//! (`agent_plugin`). The actual `AgentResource` type and its
//! `Resource` impl live in `handler.rs` + `dispatch.rs` +
//! `stream.rs`.

use std::sync::Arc;

use cordis::{plugin_with, Context, Injection, LogLevel, Plugin};

use crate::host::resolver::Reachable;
use crate::kernel::ids::PluginId;
use crate::kernel::{CapabilitySpace, Slot};
use crate::plugins::agent::handler::AgentResource;

use crate::host::resolver::ResolvedPlan;

/// Backwards-compatible constructor used by tests that build
/// the reachable set from `(handle, SlotId)` pairs and want to
/// keep the slot-id path (β.4 uses this for "same reachability,
/// different authority").
///
/// Looks up each slot's *registered* name in cspace
/// (`cspace.name_for_slot`), which for derived caps (the result
/// of `restrict`/`grant`) is the new name (e.g. `"counter_a"`)
/// rather than the cap's own `meta.name` (which inherits from
/// the parent, e.g. `"counter"`). The agent then dispatches via
/// the binding-table path (handle matching, then
/// `lookup_by_name(capability)`).
///
/// **Prefer** [`AgentResource::from_bindings`] in new tests —
/// this helper just bridges the legacy slot-id style.
pub fn handler(
    name: impl Into<String>,
    slots: Vec<(String, crate::kernel::SlotId)>,
    cspace: CapabilitySpace,
) -> Arc<AgentResource> {
    let mut entries: Vec<Reachable> = slots
        .iter()
        .map(|(handle, slot_id)| Reachable {
            handle: handle.clone(),
            capability: cspace
                .name_for_slot(*slot_id)
                .unwrap_or_else(|| format!("slot:{}", slot_id.raw())),
        })
        .collect();
    entries.sort_by(|a, b| a.handle.cmp(&b.handle));
    entries.dedup_by(|a, b| a.handle == b.handle);
    Arc::new(AgentResource::from_reachable(
        name,
        entries,
        cspace,
    ))
}

/// Capability-native constructor used by tests that have a
/// real `ResolvedPlan` (e.g. from `host::resolver::resolve`).
/// Wraps [`AgentResource::from_bindings`] and returns the
/// `Arc<AgentResource>` that `factory.mint::<AgentResource>`
/// expects.
pub fn handler_from_plan(
    name: impl Into<String>,
    plan: &ResolvedPlan,
    consumer: &PluginId,
    cspace: CapabilitySpace,
) -> Arc<AgentResource> {
    Arc::new(AgentResource::from_bindings(
        name,
        plan,
        consumer,
        cspace,
    ))
}

pub fn agent_plugin() -> Arc<dyn Plugin> {
    plugin_with(
        "agent",
        vec![Injection::from("slot:agent")],
        |ctx: Context, _cfg: ()| async move {
            let slot: Arc<Slot<AgentResource>> = ctx.require("slot:agent")?;
            ctx.logger().log(
                LogLevel::Info,
                format!(
                    "agent plugin: slot={} cap_id={}",
                    slot.id().raw(),
                    slot.capability()
                        .map(|c| c.id().to_string())
                        .unwrap_or_else(|| "(empty)".to_string()),
                ),
            );
            Ok(())
        },
    )
}
