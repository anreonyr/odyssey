//! Capability resolver — Phase 3 P3.1.
//!
//! Phase 8: this file is the merge of the Phase 5 split
//! (`error.rs` + `index.rs` + `plan.rs` + `topo.rs` + `mod.rs`)
//! into one cohesive module. The split was justified while the
//! resolver was in `host/` with several private helpers; in
//! the new layout the resolver is a personality-layer
//! computation, and one file holds the four phases
//! (error / index / topo / plan) cohesively.
//!
//! Turns a flat list of `PluginManifest`s into a `ResolvedPlan`:
//! the topological order in which plugins must mint their
//! capabilities, and the per-plugin binding table that tells
//! each plugin which slot fulfils each `[[requires]] contract`].
//!
//! Reads as `index → topo → plan`; each phase is internally
//! scoped but the four live in one file so the orchestrator
//! (`resolve`) reads linearly.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;

use crate::core::identity::ids::PluginId;
use crate::core::manifest::manifest::PluginManifest;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum ResolveError {
    /// A consumer's `[[requires]] contract` had no matching
    /// `[[exposes]] contract_name` in any other manifest.
    Unprovided {
        contract: String,
        by: String,
    },
    /// Two providers published the same contract without a
    /// priority hint; the resolver can't pick one.
    Ambiguous {
        contract: String,
        a: String,
        b: String,
    },
    /// The dependency graph has a cycle. The chain lists the
    /// plugins that form the cycle, in `"name@version"` form.
    Cycle {
        chain: Vec<String>,
    },
    DuplicateName {
        plugin: PluginId,
    },
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unprovided { contract, by } => write!(
                f,
                "no provider for contract `{contract}` (requested by {by})"
            ),
            Self::Ambiguous { contract, a, b } => {
                write!(f, "ambiguous contract `{contract}` (providers: {a}, {b})")
            }
            Self::Cycle { chain } => {
                write!(f, "dependency cycle detected ({} plugin(s))", chain.len())
            }
            Self::DuplicateName { plugin } => write!(
                f,
                "duplicate plugin name `{}@{}`",
                plugin.name, plugin.version
            ),
        }
    }
}

impl std::error::Error for ResolveError {}

// ---------------------------------------------------------------------------
// Contract index
// ---------------------------------------------------------------------------

/// Single contract-name index entry. The cap name is the
/// `[[exposes]] name` field of the provider — distinct from
/// the contract name itself (which is the resolver key).
#[derive(Debug, Clone)]
struct ContractEntry<'a> {
    plugin: PluginId,
    cap_name: &'a str,
}

/// Build a contract-name index over every manifest. Detects
/// two failure modes at construction time:
///  - `DuplicateName`: two manifests share the same `(name, version)`.
///  - `Ambiguous`: two manifests publish the same `contract_name`.
///
/// Both are boot errors — the caller surfaces them before
/// any plugin starts minting. Detecting at index-build time
/// is what lets the rest of the resolver assume "every
/// contract has exactly one provider" and run a single
/// hash-table lookup per `requires` entry.
type ContractIndex<'a> = BTreeMap<String, ContractEntry<'a>>;

fn build_contract_index(manifests: &[PluginManifest]) -> Result<ContractIndex<'_>, ResolveError> {
    let mut by_contract: BTreeMap<String, ContractEntry<'_>> = BTreeMap::new();
    // Phase 9.5 cleanup: the previous code carried a
    // `BTreeMap<PluginId, &PluginManifest>` alongside the
    // contract index. It was built, returned, then
    // immediately dropped at the call site with
    // `let _ = by_plugin;`. The map's only load-bearing
    // effect was the duplicate-name check via
    // `insert().is_some()`. Replaced with a `HashSet` so the
    // dedup logic stays (and keeps `O(1)` average for the
    // small manifest counts the orchestrator handles today)
    // without shipping the never-read `&PluginManifest`
    // payload.
    let mut seen: HashSet<PluginId> = HashSet::new();

    for m in manifests {
        let pid = m.plugin.clone();
        if !seen.insert(pid.clone()) {
            return Err(ResolveError::DuplicateName { plugin: pid });
        }
        for e in &m.exposes {
            if e.contract_name.is_empty() {
                continue;
            }
            if by_contract.contains_key(&e.contract_name) {
                let other = by_contract.get(&e.contract_name).unwrap();
                return Err(ResolveError::Ambiguous {
                    contract: e.contract_name.clone(),
                    a: format!("{}@{}", other.plugin.name, other.plugin.version),
                    b: format!("{}@{}", pid.name, pid.version),
                });
            }
            by_contract.insert(
                e.contract_name.clone(),
                ContractEntry {
                    plugin: pid.clone(),
                    cap_name: &e.name,
                },
            );
        }
    }

    Ok(by_contract)
}

// ---------------------------------------------------------------------------
// Topological sort (Kahn's algorithm with cycle detection)
// ---------------------------------------------------------------------------

/// Run Kahn's algorithm over the edge map. Returns the
/// mint order (all nodes with no in-degree first, then
/// consumers whose providers have all been emitted). If
/// the order is shorter than the node count, the remaining
/// nodes form a cycle — surface them as `ResolveError::Cycle`.
fn topological_sort(
    edges: &BTreeMap<PluginId, BTreeSet<PluginId>>,
    mut in_degree: BTreeMap<PluginId, usize>,
) -> Result<Vec<PluginId>, ResolveError> {
    let mut queue: BTreeSet<PluginId> = in_degree
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(p, _)| p.clone())
        .collect();
    let mut order = Vec::new();

    while let Some(p) = queue.iter().next().cloned() {
        queue.remove(&p);
        order.push(p.clone());
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

    if order.len() != in_degree.len() {
        let chain: Vec<String> = in_degree
            .iter()
            .filter(|(_, d)| **d > 0)
            .map(|(p, _)| format!("{}@{}", p.name, p.version))
            .collect();
        return Err(ResolveError::Cycle { chain });
    }

    Ok(order)
}

/// Add an edge `from → to` (i.e. `from` must mint before `to`).
/// Increments `to`'s in-degree if the edge was new. Idempotent
/// for repeated edges (the BTreeSet dedups them).
fn add_edge(
    edges: &mut BTreeMap<PluginId, BTreeSet<PluginId>>,
    in_degree: &mut BTreeMap<PluginId, usize>,
    from: PluginId,
    to: PluginId,
) {
    edges.entry(from.clone()).or_default();
    let inserted = edges.get_mut(&from).unwrap().insert(to.clone());
    if inserted {
        *in_degree.entry(to).or_insert(0) += 1;
    }
}

// ---------------------------------------------------------------------------
// Plan + binding types
// ---------------------------------------------------------------------------

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
/// A thin view over `ResolvedBinding`, used by every consumer
/// that needs to dispatch by capability name (`Generator`,
/// `Agent`, etc.).
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

// ---------------------------------------------------------------------------
// Orchestrator
// ---------------------------------------------------------------------------

/// Walk every manifest, build the contract index, expand the
/// `requires` edges, run the topo sort, and project the
/// result into a `ResolvedPlan`. The three failure modes
/// (`Unprovided`, `Ambiguous`, `Cycle`) all surface before
/// any plugin starts minting.
pub fn resolve(manifests: &[PluginManifest]) -> Result<ResolvedPlan, ResolveError> {
    // 1. Contract index.
    let by_contract = build_contract_index(manifests)?;

    // 2. Edges + bindings.
    let mut edges: BTreeMap<PluginId, BTreeSet<PluginId>> = BTreeMap::new();
    let mut bindings: BTreeMap<PluginId, Vec<ResolvedBinding>> = BTreeMap::new();
    let mut in_degree: BTreeMap<PluginId, usize> = BTreeMap::new();
    for m in manifests {
        let pid = m.plugin.clone();
        in_degree.entry(pid.clone()).or_insert(0);
        edges.entry(pid.clone()).or_default();
        for req in &m.requires {
            let provider: &ContractEntry<'_> =
                by_contract
                    .get(&req.contract)
                    .ok_or_else(|| ResolveError::Unprovided {
                        contract: req.contract.clone(),
                        by: pid.name.clone(),
                    })?;
            let provider_pid = provider.plugin.clone();
            let provider_cap_name = provider.cap_name;
            add_edge(
                &mut edges,
                &mut in_degree,
                provider_pid.clone(),
                pid.clone(),
            );
            bindings
                .entry(pid.clone())
                .or_default()
                .push(ResolvedBinding {
                    handle: req.name.clone(),
                    provider: provider_pid,
                    capability: provider_cap_name.to_string(),
                    contract: req.contract.clone(),
                });
        }
    }

    // 3. Topological sort.
    let mint_order = topological_sort(&edges, in_degree)?;

    Ok(ResolvedPlan {
        mint_order,
        bindings,
    })
}
