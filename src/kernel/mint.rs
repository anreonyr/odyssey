//! Mint-time helpers — turn a manifest declaration into a
//! `CapabilityMeta` ready to install in the CSpace.

use crate::capability::{CapabilityBudget, CapabilityId, CapabilityMeta};
use crate::kernel::manifest::{CapabilityDecl, PluginId};

/// Construct a `CapabilityMeta` from a manifest declaration + budget.
/// Used by the factory at mint time.
///
/// Phase 2: namespace defaults to the plugin's FQDN-style name (or the
/// declaration name if the plugin has no namespace). The contract is
/// read straight from `decl.contract` — manifests that omit it get an
/// empty contract via `CapabilityContract::default()`.
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
        timeout_ms: budget.timeout_ms,
        quota: budget.quota_state.spec(),
        authority: decl.authority.clone(),
        protocol: decl.protocol.clone(),
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