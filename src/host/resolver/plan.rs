//! Resolved plan + binding shape — Phase 5.
//!
//! `ResolvedBinding` is what the host hands to a plugin's
//! handler at mint time; the agent uses `(handle, capability)`
//! as its dispatch key. `Reachable` is the projected view of
//! a binding: just the two strings the agent needs.
//!
//! Phase 5: split from `host::reachable` so the resolver's
//! types live next to the resolver.

use std::collections::BTreeMap;

use crate::kernel::ids::PluginId;

/// One row of the per-plugin binding table. The consumer
/// (agent, generator, etc.) gets `(handle, provider,
/// capability, contract)`; the agent constructs its reachable
/// set from `(handle, capability)`.
#[derive(Debug, Clone)]
pub struct ResolvedBinding {
    /// Local handle in the consumer plugin (e.g. `"counter"`).
    pub handle: String,
    /// The plugin that published the contract. Identified by
    /// `(name, version)`.
    pub provider: PluginId,
    /// Capability name in the cspace — what `lookup_by_name`
    /// resolves against at dispatch time. Distinct from
    /// `handle` so two agents can use the same handle to
    /// reach different caps (P3.4).
    pub capability: String,
    /// Contract name that was matched. Carried for diagnostics
    /// and for any downstream type that wants to surface the
    /// capability-type vocabulary.
    pub contract: String,
}

/// `Reachable` — one row of a consumer's binding table.
///
/// Phase 5: split from `capability::reachable` and moved into
/// the resolver's plan module. It is a thin view over
/// `ResolvedBinding`, used by every consumer that needs to
/// dispatch by capability name (`Generator`, `Agent`, etc.).
///
/// `handle` is the local name the consumer uses for dispatch
/// (the input's `target` field, or the model's lookup key);
/// `capability` is the cspace name `lookup_by_name` resolves
/// against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reachable {
    pub handle: String,
    pub capability: String,
}

impl Reachable {
    pub fn from_binding(b: &ResolvedBinding) -> Self {
        Self {
            handle: b.handle.clone(),
            capability: b.capability.clone(),
        }
    }

    pub fn new(handle: impl Into<String>, capability: impl Into<String>) -> Self {
        Self {
            handle: handle.into(),
            capability: capability.into(),
        }
    }
}

/// Resolved plan — the resolver's output. `mint_order` is the
/// topological order in which plugins must mint their
/// capabilities; `bindings` is the per-plugin table that tells
/// each consumer which slot fulfils each `[[requires]] contract`.
#[derive(Debug, Clone)]
pub struct ResolvedPlan {
    pub mint_order: Vec<PluginId>,
    pub bindings: BTreeMap<PluginId, Vec<ResolvedBinding>>,
}

impl ResolvedPlan {
    /// Human-readable rendering for boot diagnostics. Lists
    /// the mint order, then per-plugin binding tables.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("Mint order:\n");
        for p in &self.mint_order {
            out.push_str(&format!("  - {}@{}\n", p.name, p.version));
        }
        out.push_str("\nBindings:\n");
        for (plugin, bindings) in &self.bindings {
            out.push_str(&format!("  {}@{}:\n", plugin.name, plugin.version));
            for b in bindings {
                out.push_str(&format!(
                    "    handle={} <- {}@{}::{}\n",
                    b.handle, b.provider.name, b.provider.version, b.capability
                ));
            }
        }
        out
    }
}
