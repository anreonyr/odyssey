//! Capability resolver — Phase 3 P3.1.
//!
//! Turns a flat list of [`PluginManifest`]s into a [`ResolvedPlan`]:
//! the topological order in which plugins must mint their
//! capabilities, and the per-plugin binding table that tells
//! each plugin which slot fulfils each `[[requires]] contract`].
//!
//! Phase 5: split from `kernel::resolver`. The Phase 4
//! `kernel::registry` is folded in here — dedup is a side
//! effect of building the contract index, so a separate
//! type is redundant.
//!
//! The resolver is a thin orchestrator over three sub-modules:
//!
//! - [`error`] — the typed `ResolveError` variants.
//! - [`index`] — contract-name → provider index, plus
//!   `DuplicateName` / `Ambiguous` detection.
//! - [`topo`] — Kahn's algorithm with cycle detection.
//! - [`plan`] — the `ResolvedPlan` output shape +
//!   `ResolvedBinding` row + `Reachable` projection.
//!
//! Reads as `index → topo → plan`; each phase is independently
//! testable.

pub mod error;
pub mod index;
pub mod plan;
pub mod topo;

pub use error::ResolveError;
pub use plan::{Reachable, ResolvedBinding, ResolvedPlan};

use std::collections::{BTreeMap, BTreeSet};

use crate::host::manifest::PluginManifest;
use crate::kernel::ids::PluginId;

use self::index::{build_contract_index, ContractEntry};
use self::plan::ResolvedBinding as Binding;
use self::topo::{add_edge, topological_sort};

/// Walk every manifest, build the contract index, expand the
/// `requires` edges, run the topo sort, and project the
/// result into a `ResolvedPlan`. The three failure modes
/// (`Unprovided`, `Ambiguous`, `Cycle`) all surface before
/// any plugin starts minting.
pub fn resolve(manifests: &[PluginManifest]) -> Result<ResolvedPlan, ResolveError> {
    // 1. Contract index.
    let (by_contract, by_plugin) = build_contract_index(manifests)?;

    // 2. Edges + bindings.
    let mut edges: BTreeMap<PluginId, BTreeSet<PluginId>> = BTreeMap::new();
    let mut bindings: BTreeMap<PluginId, Vec<Binding>> = BTreeMap::new();
    let mut in_degree: BTreeMap<PluginId, usize> = BTreeMap::new();
    for m in manifests {
        let pid = m.plugin.clone();
        in_degree.entry(pid.clone()).or_insert(0);
        edges.entry(pid.clone()).or_default();
        for req in &m.requires {
            let provider: &ContractEntry<'_> =
                by_contract.get(&req.contract).ok_or_else(|| {
                    ResolveError::Unprovided {
                        contract: req.contract.clone(),
                        by: pid.name.clone(),
                    }
                })?;
            let provider_pid = provider.plugin.clone();
            let provider_cap_name = provider.cap_name;
            add_edge(&mut edges, &mut in_degree, provider_pid.clone(), pid.clone());
            bindings
                .entry(pid.clone())
                .or_default()
                .push(Binding {
                    handle: req.name.clone(),
                    provider: provider_pid,
                    capability: provider_cap_name.to_string(),
                    contract: req.contract.clone(),
                });
        }
    }

    // 3. Topological sort.
    let mint_order = topological_sort(&edges, in_degree)?;

    // Touch by_plugin so the unused warning stays quiet —
    // the index build populates it as a side effect and we
    // may consult it in a later phase (e.g. for capability
    // type validation against the manifest).
    let _ = by_plugin;

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
