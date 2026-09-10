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

use std::collections::{BTreeMap, BTreeSet};

use crate::host::manifest::PluginManifest;
use crate::kernel::ids::PluginId;

#[derive(Debug)]
pub enum ResolveError {
    /// A consumer's `[[requires]] contract` had no matching
    /// `[[exposes]] contract_name` in any other manifest.
    Unprovided { contract: String, by: String },
    /// Two providers published the same contract without a
    /// priority hint; the resolver can't pick one.
    Ambiguous { contract: String, a: String, b: String },
    /// The dependency graph has a cycle. The chain lists the
    /// plugins that form the cycle, in `"name@version"` form.
    Cycle { chain: Vec<String> },
    DuplicateName { plugin: PluginId },
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unprovided { contract, by } => write!(
                f,
                "no provider for contract `{contract}` (requested by {by})"
            ),
            Self::Ambiguous { contract, a, b } => write!(
                f,
                "ambiguous contract `{contract}` (providers: {a}, {b})"
            ),
            Self::Cycle { chain } => write!(
                f,
                "dependency cycle detected ({} plugin(s))",
                chain.len()
            ),
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

#[derive(Debug, Clone)]
pub struct ResolvedPlan {
    pub mint_order: Vec<PluginId>,
    pub bindings: BTreeMap<PluginId, Vec<ResolvedBinding>>,
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
                match by_contract.get(&e.contract_name).cloned() {
                    Some((other_pid, _, _)) => {
                        // Ambiguous: two plugins publish the same
                        // contract. Surface the first two providers
                        // by `"name@version"` for diagnostics.
                        return Err(ResolveError::Ambiguous {
                            contract: e.contract_name.clone(),
                            a: format!("{}@{}", other_pid.name, other_pid.version),
                            b: format!("{}@{}", pid.name, pid.version),
                        });
                    }
                    None => {
                        by_contract.insert(
                            e.contract_name.clone(),
                            (pid.clone(), m, &e.name),
                        );
                    }
                }
            }
        }
    }

    // 2. Edges + bindings.
    let mut edges: BTreeMap<PluginId, BTreeSet<PluginId>> = BTreeMap::new();
    let mut bindings: BTreeMap<PluginId, Vec<ResolvedBinding>> = BTreeMap::new();
    let mut in_degree: BTreeMap<PluginId, usize> = BTreeMap::new();
    for m in manifests {
        let pid = m.plugin.clone();
        in_degree.entry(pid.clone()).or_insert(0);
        edges.entry(pid.clone()).or_default();
        for req in &m.requires {
            let provider = by_contract.get(&req.contract).ok_or_else(|| {
                ResolveError::Unprovided {
                    contract: req.contract.clone(),
                    by: pid.name.clone(),
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
                    handle: req.name.clone(),
                    provider: provider_pid,
                    capability: (*provider_cap_name).to_string(),
                    contract: req.contract.clone(),
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
        let chain: Vec<String> = in_degree
            .iter()
            .filter(|(_, d)| **d > 0)
            .map(|(p, _)| format!("{}@{}", p.name, p.version))
            .collect();
        return Err(ResolveError::Cycle { chain });
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
                contract_name: contract.into(),
                in_type: "any".into(),
                out_type: "any".into(),
                streaming: false,
                authority: crate::kernel::meta::AuthorityContract::default(),
                protocol: crate::kernel::meta::Protocol::default(),
            }],
            requires: requires
                .into_iter()
                .map(|(n, c)| crate::host::manifest::CapabilityRequirement {
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
        assert!(matches!(plan, Err(ResolveError::Cycle { .. })));
    }

    #[test]
    fn unprovided_contract_errors() {
        // Plugin `a` requires a contract that no other manifest publishes.
        // The resolver must surface the gap as `ResolveError::Unprovided`.
        let a = make_manifest("a", "echo", vec![("missing", "missing")]);
        let plan = resolve(&[a]);
        assert!(
            matches!(plan, Err(ResolveError::Unprovided { .. })),
            "expected Unprovided error, got {plan:?}"
        );
    }
}
