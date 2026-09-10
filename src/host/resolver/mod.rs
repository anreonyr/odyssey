//! Capability resolver — Phase 3 P3.1.
//!
//! Turns a flat list of [`PluginManifest`]s into a [`ResolvedPlan`]:
//! the topological order in which plugins must mint their
//! capabilities, and the per-plugin binding table that tells
//! each plugin which [`crate::kernel::SlotId`] fulfils each
//! `[[requires]] contract`.
//!
//! Phase 5: split from `kernel::resolver`. The Phase 4
//! `kernel::registry` is folded in here — dedup is a side
//! effect of building the contract index, so a separate type
//! is redundant.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::host::manifest::{CapabilityRequirement, PluginManifest};
use crate::kernel::ids::PluginId;
use crate::kernel::ids::SlotId;

#[derive(Debug)]
pub enum ResolveError {
    Unprovided { contract: String, requested_by: PluginId },
    Ambiguous { contract: String, providers: Vec<PluginId> },
    Cycle(Vec<PluginId>),
    DuplicateName { plugin: PluginId },
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unprovided { contract, requested_by } => write!(
                f,
                "no provider for contract `{contract}` (requested by {}@{})",
                requested_by.name, requested_by.version
            ),
            Self::Ambiguous { contract, providers } => write!(
                f,
                "ambiguous contract `{contract}` (providers: {})",
                providers.len()
            ),
            Self::Cycle(cycle) => write!(f, "dependency cycle detected ({} plugin(s))", cycle.len()),
            Self::DuplicateName { plugin } => write!(
                f,
                "duplicate plugin name `{}@{}`",
                plugin.name, plugin.version
            ),
        }
    }
}

impl std::error::Error for ResolveError {}

#[derive(Debug, Clone)]
pub struct ResolvedBinding {
    pub requirement: CapabilityRequirement,
    pub provider: PluginId,
    pub provider_slot: SlotId,
    /// Phase 5: this is the `Reachable` shape. ResolvedBindings
    /// are what the host hands to a plugin's handler at mint
    /// time. The plugin gets `(requirement.name, ResolvedBinding)`;
    /// it can use `ResolvedBinding.provider_slot` to deref the cap.
    pub contract: String,
    /// Local handle in the consumer plugin (matches `requirement.name`).
    /// Convenience for `Reachable::from_binding`.
    pub handle: String,
    /// Capability name the consumer dispatches by. Matches the
    /// `[[exposes]] name` of the provider. Convenience for
    /// `Reachable::from_binding`.
    pub capability: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedPlan {
    pub mint_order: Vec<PluginId>,
    pub bindings: HashMap<PluginId, Vec<ResolvedBinding>>,
}

impl ResolvedPlan {
    /// Human-readable rendering for boot diagnostics. Lists the
    /// mint order, then per-plugin binding tables.
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

pub fn resolve(manifests: &[PluginManifest]) -> Result<ResolvedPlan, ResolveError> {
    // 1. Contract index.
    let mut by_contract: BTreeMap<String, (PluginId, &PluginManifest, &str)> = BTreeMap::new();
    let mut by_plugin: BTreeMap<PluginId, &PluginManifest> = BTreeMap::new();
    for m in manifests {
        let pid = m.plugin.clone();
        if by_plugin.insert(pid.clone(), m).is_some() {
            return Err(ResolveError::DuplicateName { plugin: pid });
        }
        for e in &m.exposes {
            if !e.contract_name.is_empty() {
                if by_contract.insert(e.contract_name.clone(), (pid.clone(), m, &e.name)).is_some() {
                    // Ambiguous: two plugins publish the same contract.
                    return Err(ResolveError::Ambiguous {
                        contract: e.contract_name.clone(),
                        providers: by_contract
                            .values()
                            .filter(|(p, _, _)| p == &pid)
                            .map(|(p, _, _)| p.clone())
                            .collect(),
                    });
                }
            }
        }
    }

    // 2. Edges + bindings.
    let mut edges: BTreeMap<PluginId, BTreeSet<PluginId>> = BTreeMap::new();
    let mut bindings: HashMap<PluginId, Vec<ResolvedBinding>> = HashMap::new();
    let mut in_degree: BTreeMap<PluginId, usize> = BTreeMap::new();
    for m in manifests {
        let pid = m.plugin.clone();
        in_degree.entry(pid.clone()).or_insert(0);
        edges.entry(pid.clone()).or_default();
        for req in &m.requires {
            let provider = by_contract.get(&req.contract).ok_or_else(|| {
                ResolveError::Unprovided {
                    contract: req.contract.clone(),
                    requested_by: pid.clone(),
                }
            })?;
            let (provider_pid, _provider_manifest, provider_cap_name) = provider;
            let provider_pid = provider_pid.clone();
            edges.entry(provider_pid.clone()).or_default();
            if edges.get_mut(&provider_pid).unwrap().insert(pid.clone()) {
                *in_degree.entry(pid.clone()).or_insert(0) += 1;
            }
            bindings
                .entry(pid.clone())
                .or_default()
                .push(ResolvedBinding {
                    requirement: req.clone(),
                    provider: provider_pid,
                    provider_slot: SlotId::new(0), // assigned at mint time
                    contract: (*provider_cap_name).to_string(),
                    handle: req.name.clone(),
                    capability: (*provider_cap_name).to_string(),
                });
        }
    }

    // 3. Topological sort (Kahn's algorithm).
    let mut queue: std::collections::BTreeSet<PluginId> = in_degree
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(p, _)| p.clone())
        .collect();
    let mut mint_order = Vec::new();
    while let Some(p) = queue.iter().next().cloned() {
        queue.remove(&p);
        mint_order.push(p.clone());
        if let Some(succs) = edges.get(&p) {
            for s in succs {
                if let Some(d) = in_degree.get_mut(s) {
                    *d = d.saturating_sub(1);
                    if *d == 0 {
                        queue.insert(s.clone());
                    }
                }
            }
        }
    }
    if mint_order.len() != in_degree.len() {
        let cycle: Vec<PluginId> = in_degree
            .iter()
            .filter(|(_, d)| **d > 0)
            .map(|(p, _)| p.clone())
            .collect();
        return Err(ResolveError::Cycle(cycle));
    }

    Ok(ResolvedPlan { mint_order, bindings })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::manifest::{CapabilityDecl, IsolationMode};

    fn make_manifest(name: &str, contract: &str, requires: Vec<(&str, &str)>) -> PluginManifest {
        PluginManifest {
            plugin: PluginId { name: name.into(), version: "0.1.0".into() },
            isolate: IsolationMode::InProc,
            exposes: vec![CapabilityDecl {
                name: name.into(),
                contract: contract.into(),
                in_type: "any".into(),
                out_type: "any".into(),
                streaming: false,
                quota: None,
            }],
            requires: requires
                .into_iter()
                .map(|(n, c)| CapabilityRequirement {
                    name: n.into(),
                    contract: c.into(),
                })
                .collect(),
            consumes: vec![],
            host: vec![],
            resources: Default::default(),
        }
    }

    #[test]
    fn single_provider_resolves() {
        let m = make_manifest("a", "echo", vec![]);
        let plan = resolve(&[m]).unwrap();
        assert_eq!(plan.mint_order.len(), 1);
    }

    #[test]
    fn linear_chain_resolves() {
        let a = make_manifest("a", "echo", vec![]);
        let b = make_manifest("b", "transform", vec![("echo", "echo")]);
        let plan = resolve(&[a, b]).unwrap();
        assert_eq!(plan.mint_order[0].name, "a");
        assert_eq!(plan.mint_order[1].name, "b");
    }

    #[test]
    fn cycle_is_detected() {
        let a = make_manifest("a", "echo", vec![("loop", "loop")]);
        let b = make_manifest("b", "loop", vec![("echo", "echo")]);
        let plan = resolve(&[a, b]);
        assert!(matches!(plan, Err(ResolveError::Cycle(_))));
    }

    #[test]
    fn unprovided_contract_errors() {
        let a = make_manifest("a", "missing", vec![]);
        let plan = resolve(&[a]);
        assert!(matches!(plan, Err(ResolveError::DuplicateName { .. }) | Err(ResolveError::Ambiguous { .. })));
    }
}
