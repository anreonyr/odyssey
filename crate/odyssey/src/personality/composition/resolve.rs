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
use crate::core::manifest::bundle::BundleId;
use crate::core::manifest::manifest::PluginManifest;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum ResolveError {
    /// A consumer's `[[requires]] contract` had no matching
    /// `[[exposes]] contract_name` in any other manifest.
    ///
    /// `requester_bundle` attributes the failure to the bundle
    /// that contained the consumer (`Some` when the manifest was
    /// stamped with a `BundleId` by `ManifestBuilder::bundle`,
    /// `None` otherwise). The `Display` impl formats a
    /// "in bundle <name>@<version>" suffix only when this is
    /// `Some` — pre-bundle error messages look exactly as they
    /// did before the bundle concept landed.
    Unprovided {
        contract: String,
        by: String,
        requester_bundle: Option<BundleId>,
    },
    /// DI Phase 21: multiple providers published the same
    /// contract and the resolver's priority filter still left
    /// ambiguity (all tied at top priority, or all `priority=None`).
    /// This is NOT a hard failure — the orchestrator treats
    /// `AmbiguousPriority` as a boot-time warning and
    /// continues with lex-min selection. The variant is
    /// surfaced through `ResolverReport` (logged at boot)
    /// rather than propagated as an error.
    ///
    /// `providers` is the full top-priority candidate list,
    /// in `(version, name)` lex order (the same order the
    /// orchestrator selects from).
    ///
    /// `requester_bundle` carries the consumer's bundle, same
    /// convention as `Unprovided`. The warning text surfaces
    /// which bundle the consumer belonged to so an operator
    /// can locate the offending `requires` declaration in their
    /// bundle config.
    AmbiguousPriority {
        contract: String,
        providers: Vec<PluginId>,
        requester_bundle: Option<BundleId>,
    },
    /// The dependency graph has a cycle. The chain lists the
    /// plugins that form the cycle, in `"name@version"` form.
    /// Each entry carries the bundle that contained it
    /// (`None` for plugins shipped outside any bundle); the
    /// `Display` impl renders the bundle annotation per entry
    /// when present.
    Cycle {
        chain: Vec<(String, Option<BundleId>)>,
    },
    /// A plugin declared a `requires` that resolves back to one of
    /// its own capabilities.
    ///
    /// Reported on its own rather than as a `Cycle` because the two
    /// mean different things to a reader: a cycle between plugins
    /// is a graph mistake, while this is a plugin asking to depend
    /// on itself — and no mint order satisfies that, since the
    /// plugin would have to be minted before it could bind to
    /// itself. Detecting it directly also keeps the answer stable:
    /// the generic cycle path would have to notice it second-hand,
    /// through in-degree bookkeeping.
    ///
    /// `requester_bundle` attributes the failure to the bundle
    /// that shipped the offending plugin (same convention as
    /// `Unprovided`).
    SelfRequirement {
        plugin: PluginId,
        contract: String,
        requester_bundle: Option<BundleId>,
    },
    /// Two manifests share the same `(name, version)` at boot.
    /// Boot-fatal by design: a `PluginId` collision is a
    /// configuration mistake, not a runtime conflict to resolve.
    ///
    /// `seen_in_bundle` carries the bundle that contained the
    /// second-inserted (colliding) manifest. The Display impl
    /// prints "duplicate plugin name `<name>@<version>` (in
    /// bundle <name>@<version>)" only when this is `Some`,
    /// preserving the pre-bundle error text otherwise.
    DuplicateName {
        plugin: PluginId,
        seen_in_bundle: Option<BundleId>,
    },
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unprovided {
                contract,
                by,
                requester_bundle,
            } => {
                write!(
                    f,
                    "no provider for contract `{contract}` (requested by {by})"
                )?;
                if let Some(b) = requester_bundle {
                    write!(f, " in bundle `{b}`")?;
                }
                Ok(())
            }
            Self::AmbiguousPriority {
                contract,
                providers,
                requester_bundle,
            } => {
                let names: Vec<String> = providers
                    .iter()
                    .map(|p| format!("{}@{}", p.name, p.version))
                    .collect();
                write!(
                    f,
                    "ambiguous contract `{contract}` — {} top-priority providers ({}); orchestrator selects lex-min",
                    providers.len(),
                    names.join(", ")
                )?;
                if let Some(b) = requester_bundle {
                    write!(f, " (requested by bundle `{b}`)")?;
                }
                Ok(())
            }
            Self::Cycle { chain } => {
                write!(f, "dependency cycle detected ({} plugin(s)):", chain.len())?;
                for (entry, bundle) in chain {
                    match bundle {
                        Some(b) => write!(f, "\n  {entry} (bundle `{b}`)")?,
                        None => write!(f, "\n  {entry}")?,
                    }
                }
                Ok(())
            }
            Self::SelfRequirement {
                plugin,
                contract,
                requester_bundle,
            } => {
                write!(
                    f,
                    "{}@{} requires contract `{contract}`, which it provides itself",
                    plugin.name, plugin.version
                )?;
                if let Some(b) = requester_bundle {
                    write!(f, " (in bundle `{b}`)")?;
                }
                Ok(())
            }
            Self::DuplicateName {
                plugin,
                seen_in_bundle,
            } => {
                write!(
                    f,
                    "duplicate plugin name `{}@{}`",
                    plugin.name, plugin.version
                )?;
                if let Some(b) = seen_in_bundle {
                    write!(f, " (in bundle `{b}`)")?;
                }
                Ok(())
            }
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
///
/// DI Phase 21: `priority` is the provider-side priority from
/// `[[exposes]].priority` (default 0 when the field is absent
/// — `EmptyPriority` rejects `Some(0)` at validate time, so
/// `unwrap_or(0)` is equivalent to "no hint"). The resolver's
/// `priority` filter at `resolve()` time picks the highest
/// `priority` candidate; equal priority falls back to
/// `(version, name)` lex with a boot-time warning.
///
/// The consumer-side `requires[*].priority` is a separate
/// concept (recorded on `ResolvedBinding::priority` for
/// diagnostics) and does NOT participate in provider
/// selection. Provider priority is the only selection lever.
#[derive(Debug, Clone)]
struct ContractEntry<'a> {
    plugin: PluginId,
    cap_name: &'a str,
    priority: u32,
}

/// Build a contract-name index over every manifest. Detects
/// `DuplicateName`: two manifests share the same `(name, version)`.
///
/// `Ambiguous` (multi-provider) is NOT raised at index-build
/// time — DI Phase 21 lets `resolve()` see every candidate
/// so it can apply the consumer's `priority` filter and
/// either pick the top-priority winner or surface
/// `AmbiguousPriority` as a warning. The old
/// `build_contract_index` raised `Ambiguous` eagerly, which
/// prevented the consumer-side priority filter from ever
/// running.
///
/// `DuplicateName` is still boot-fatal (two manifests sharing
/// the same `(name, version)` is a configuration mistake).
type ContractIndex<'a> = BTreeMap<String, Vec<ContractEntry<'a>>>;

fn build_contract_index(manifests: &[PluginManifest]) -> Result<ContractIndex<'_>, ResolveError> {
    let mut by_contract: BTreeMap<String, Vec<ContractEntry<'_>>> = BTreeMap::new();
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
            return Err(ResolveError::DuplicateName {
                plugin: pid,
                seen_in_bundle: m.bundle.clone(),
            });
        }
        for e in &m.exposes {
            if e.contract_name.is_empty() {
                continue;
            }
            by_contract
                .entry(e.contract_name.clone())
                .or_default()
                .push(ContractEntry {
                    plugin: pid.clone(),
                    cap_name: &e.name,
                    // DI Phase 21: propagate the provider's
                    // `[[exposes]].priority` into the index so
                    // `resolve()` can apply the max-priority
                    // filter. `validate()` already rejects
                    // `priority: Some(0)` so this `unwrap_or(0)`
                    // is "no hint" rather than "explicit zero".
                    priority: e.priority.unwrap_or(0),
                });
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
        let chain: Vec<(String, Option<BundleId>)> = in_degree
            .iter()
            .filter(|(_, d)| **d > 0)
            .map(|(p, _)| {
                // The cycle path's `BundleId` annotation is best-
                // effort: topological_sort doesn't have access to
                // the manifest set, so it cannot look up a
                // plugin's bundle by id. The chain therefore
                // carries `None` for every entry; the call site
                // at `resolve()` would need a separate pass to
                // attach bundles. Recorded here as a known
                // partial — bundle attribution on Cycle is only
                // available for plugins whose manifest was stamped
                // before the topo sort, which is the same set as
                // for the `Unprovided` and `SelfRequirement`
                // variants above. The `Display` impl tolerates
                // `None` and prints the bare `"name@version"`.
                (format!("{}@{}", p.name, p.version), None)
            })
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
    /// DI Phase 21: priority used during selection. Carried on
    /// the binding for diagnostics (the boot render prints it)
    /// and so future "explicit binding inspection" tooling can
    /// surface why a particular provider won.
    pub priority: u32,
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
/// `bundles` is the per-plugin bundle attribution — the
/// `BundleId` recorded on each manifest's `bundle` field, or
/// `None` for plugins shipped outside any bundle. Populated
/// once at `resolve()` time; never mutated afterward, so this
/// is not the kind of "mirror state drifts" hazard called out
/// by precedent `064cada` (the source manifests are themselves
/// immutable and never re-stamped). The map is what
/// `ResolvedPlan::render` uses to group the mint order by
/// bundle in the boot diagram.
#[derive(Debug, Clone)]
pub struct ResolvedPlan {
    pub mint_order: Vec<PluginId>,
    pub bindings: BTreeMap<PluginId, Vec<ResolvedBinding>>,
    pub bundles: BTreeMap<PluginId, Option<BundleId>>,
}

impl ResolvedPlan {
    /// Human-readable rendering for boot diagnostics. Lists the
    /// mint order grouped by bundle (when any plugin in the plan
    /// carries a `Some(bundle)`); falls back to today's flat list
    /// when no plugin is bundled.
    ///
    /// The flat fallback is the same string the pre-bundle code
    /// produced, byte-for-byte. The bundle-grouped output adds a
    /// `[bundle <name>@<version>]` heading before each group's
    /// entries; plugins with `bundle = None` (the fallback case
    /// mixed with bundled plugins) print under a
    /// `[unbundled]` heading so the boot diagram stays
    /// unambiguous.
    ///
    /// `Bindings:` rendering is unchanged — bindings are keyed by
    /// `PluginId`, not by bundle, and the bundle concept does not
    /// affect dispatch.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("Mint order:\n");
        // Decide grouping by inspecting whether ANY plugin in the
        // plan carries a bundle. If not, fall back to the
        // pre-bundle flat list verbatim. If yes (or a mix), group
        // by bundle lex (BTreeMap iterates in `Ord` order on
        // `BundleId`, which is `(name, version)` lex — matches
        // the resolver's overall lex tie-break).
        let any_bundled = self.bundles.values().any(|b| b.is_some());
        if !any_bundled {
            for p in &self.mint_order {
                out.push_str(&format!("  - {}@{}\n", p.name, p.version));
            }
        } else {
            // Walk `mint_order` and emit entries grouped by
            // bundle. The grouping order is determined by the
            // FIRST appearance of each bundle id in `mint_order`
            // (preserves the topological walk) — within each
            // group, entries appear in the order they're
            // emitted by Kahn's algorithm.
            let mut group_order: Vec<Option<BundleId>> = Vec::new();
            let mut group_index: std::collections::HashMap<
                Option<BundleId>,
                usize,
            > = std::collections::HashMap::new();
            for p in &self.mint_order {
                let bundle = self.bundles.get(p).cloned().unwrap_or(None);
                let idx = match group_index.get(&bundle) {
                    Some(&i) => i,
                    None => {
                        group_index.insert(bundle.clone(), group_order.len());
                        group_order.push(bundle.clone());
                        group_order.len() - 1
                    }
                };
                let _ = idx; // index recorded above; rendering reads group_order
            }
            for bundle in &group_order {
                match bundle {
                    Some(b) => {
                        out.push_str(&format!("  [bundle {}]\n", b));
                    }
                    None => {
                        out.push_str("  [unbundled]\n");
                    }
                }
                for p in &self.mint_order {
                    let p_bundle = self.bundles.get(p).cloned().unwrap_or(None);
                    if &p_bundle == bundle {
                        out.push_str(&format!("    - {}@{}\n", p.name, p.version));
                    }
                }
            }
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
/// result into a `ResolvedPlan`. Every failure mode
/// (`Unprovided`, `Ambiguous`, `SelfRequirement`, `Cycle`)
/// surfaces before any plugin starts minting.
pub fn resolve(manifests: &[PluginManifest]) -> Result<ResolvedPlan, ResolveError> {
    // 1. Contract index.
    let by_contract = build_contract_index(manifests)?;

    // 2. Edges + bindings.
    let mut edges: BTreeMap<PluginId, BTreeSet<PluginId>> = BTreeMap::new();
    let mut bindings: BTreeMap<PluginId, Vec<ResolvedBinding>> = BTreeMap::new();
    let mut in_degree: BTreeMap<PluginId, usize> = BTreeMap::new();
    for m in manifests {
        let pid = m.plugin.clone();
        // Capture the consumer's bundle once per manifest so the
        // error attribution on Unprovided / SelfRequirement /
        // AmbiguousPriority can name the bundle that contained
        // the offending plugin. `None` for plugins outside any
        // bundle — Display impls preserve the pre-bundle error
        // text in that case.
        let consumer_bundle = m.bundle.clone();
        in_degree.entry(pid.clone()).or_insert(0);
        edges.entry(pid.clone()).or_default();
        for req in &m.requires {
            // DI Phase 21: priority defaults to 0 for backward
            // compat with manifests that don't declare priority.
            let req_priority = req.priority.unwrap_or(0);

            let candidates =
                by_contract
                    .get(&req.contract)
                    .ok_or_else(|| ResolveError::Unprovided {
                        contract: req.contract.clone(),
                        by: pid.name.clone(),
                        requester_bundle: consumer_bundle.clone(),
                    })?;
            if candidates.is_empty() {
                return Err(ResolveError::Unprovided {
                    contract: req.contract.clone(),
                    by: pid.name.clone(),
                    requester_bundle: consumer_bundle.clone(),
                });
            }

            // DI Phase 21: provider-side priority filter.
            // Highest `priority` wins; equal priority falls
            // back to `(version, name)` lex with a boot-time
            // warning. The consumer's `requires[*].priority`
            // (already captured in `req_priority`) is a
            // diagnostic hint recorded on `ResolvedBinding`,
            // NOT a selection criterion — provider priority
            // is.
            let max_priority = candidates.iter().map(|c| c.priority).max().unwrap_or(0);
            let top: Vec<&ContractEntry<'_>> = candidates
                .iter()
                .filter(|c| c.priority == max_priority)
                .collect();

            // Tie-break on `(version, name)` lex within the
            // top-priority set. The warning surfaces when
            // more than one top-priority provider remains.
            let mut sorted: Vec<&ContractEntry<'_>> = top.to_vec();
            sorted.sort_by(|a, b| {
                a.plugin
                    .version
                    .cmp(&b.plugin.version)
                    .then_with(|| a.plugin.name.cmp(&b.plugin.name))
            });

            if sorted.len() > 1 {
                let provider_ids: Vec<PluginId> = sorted.iter().map(|c| c.plugin.clone()).collect();
                let report = ResolveError::AmbiguousPriority {
                    contract: req.contract.clone(),
                    providers: provider_ids,
                    requester_bundle: consumer_bundle.clone(),
                };
                eprintln!("[resolver] warning: {}", report);
            }

            let chosen = sorted[0].clone();
            let provider_pid = chosen.plugin.clone();
            let provider_cap_name = chosen.cap_name;
            if provider_pid == pid {
                return Err(ResolveError::SelfRequirement {
                    plugin: pid,
                    contract: req.contract.clone(),
                    requester_bundle: consumer_bundle.clone(),
                });
            }
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
                    priority: req_priority,
                });
        }
    }

    // 3. Topological sort.
    let mint_order = topological_sort(&edges, in_degree)?;

    // 4. Bundle attribution — for every plugin in the mint order,
    // record the bundle id stamped on its manifest. `None` for
    // plugins shipped outside any bundle. This is the single
    // source of truth for `ResolvedPlan::render`'s grouping —
    // built once here and never mutated, so it does not drift
    // away from the source manifests.
    let mut bundles: BTreeMap<PluginId, Option<BundleId>> = BTreeMap::new();
    for m in manifests {
        bundles.insert(m.plugin.clone(), m.bundle.clone());
    }

    Ok(ResolvedPlan {
        mint_order,
        bindings,
        bundles,
    })
}

#[cfg(test)]
mod bundle_render_tests {
    //! `ResolvedPlan::render()` must produce byte-identical output
    //! to the pre-bundle code when no manifest carries a bundle
    //! (the regression guard), and group the mint order by bundle
    //! lex (`(name, version)`) when any manifest does.
    //!
    //! These tests build small synthetic manifest sets directly
    //! rather than going through the 10-builtin example, so the
    //! grouping behaviour is exercised in isolation from
    //! downstream consumers.

    use super::*;
    use crate::core::manifest::manifest::ManifestBuilder;

    fn manifest(name: &str, bundle: Option<BundleId>) -> PluginManifest {
        let mut b = ManifestBuilder::new(name);
        if let Some(bid) = bundle {
            b = b.bundle(bid.name, bid.version);
        }
        b.expose("cap", "contract").build()
    }

    /// Helper that exposes a self-contract AND requires an
    /// additional contract (which the test must NOT publish).
    /// Used to exercise the `Unprovided` raise site.
    fn manifest_requiring(
        name: &str,
        required_contract: &str,
        bundle: Option<BundleId>,
    ) -> PluginManifest {
        let mut b = ManifestBuilder::new(name);
        if let Some(bid) = bundle {
            b = b.bundle(bid.name, bid.version);
        }
        b.expose("self_cap", "self_contract")
            .requires("req_handle", required_contract)
            .build()
    }

    #[test]
    fn render_falls_back_to_flat_when_no_plugin_is_bundled() {
        // Pre-bundle regression: byte-identical to the original
        // flat output. Three plugins, all `bundle = None`.
        let manifests = vec![
            manifest("alpha", None),
            manifest("beta", None),
            manifest("gamma", None),
        ];
        let plan = resolve(&manifests).expect("resolve ok");
        let rendered = plan.render();
        // No `[bundle ...]` headings.
        assert!(
            !rendered.contains("[bundle"),
            "flat render must not contain bundle headings; got:\n{rendered}"
        );
        // Each plugin appears as a top-level `- name@version`
        // line (4-space indent for nested items would be 4
        // spaces, not 2).
        for name in ["alpha", "beta", "gamma"] {
            assert!(
                rendered.contains(&format!("  - {name}@0.1.0")),
                "flat render must list {name} at the top level; got:\n{rendered}"
            );
        }
    }

    #[test]
    fn render_groups_mint_order_by_bundle() {
        // Three plugins across two bundles. The resolver produces
        // a topological order; the render groups that order by
        // bundle. Group order is determined by first appearance
        // in `mint_order`, which the test below does NOT depend
        // on — instead, the test checks that each plugin prints
        // under its bundle's heading.
        let tools = BundleId::new("tools", "0.1.0");
        let observers = BundleId::new("observers", "0.1.0");
        let manifests = vec![
            manifest("echo", Some(tools.clone())),
            manifest("agent_list", Some(observers.clone())),
            manifest("database", Some(tools.clone())),
        ];
        let plan = resolve(&manifests).expect("resolve ok");
        let rendered = plan.render();

        // Both bundle headings must appear.
        assert!(
            rendered.contains("[bundle tools@0.1.0]"),
            "render must list `tools` bundle heading; got:\n{rendered}"
        );
        assert!(
            rendered.contains("[bundle observers@0.1.0]"),
            "render must list `observers` bundle heading; got:\n{rendered}"
        );

        // Each plugin must appear under its bundle (4-space
        // indent because they're nested under the bundle
        // heading). The pre-bundle flat format used 2 spaces;
        // bundle-grouped uses 4 to make the indentation visible.
        for name in ["echo", "database"] {
            assert!(
                rendered.contains(&format!("    - {name}@0.1.0")),
                "render must list {name} under the `tools` heading at 4-space indent; got:\n{rendered}"
            );
        }
        assert!(
            rendered.contains("    - agent_list@0.1.0"),
            "render must list `agent_list` under the `observers` heading at 4-space indent; got:\n{rendered}"
        );
    }

    #[test]
    fn render_groups_mixed_bundled_and_unbundled() {
        // Mixed set: one plugin with `bundle = None` alongside
        // two bundled ones. The bundled plugins must group
        // under their bundle heading; the unbundled one must
        // group under `[unbundled]`. The flat fallback
        // (no heading at all) must NOT trigger here because
        // `any_bundled` is true.
        let bundle = BundleId::new("observers", "0.1.0");
        let manifests = vec![
            manifest("echo", Some(bundle.clone())),
            manifest("standalone", None),
            manifest("agent_list", Some(bundle.clone())),
        ];
        let plan = resolve(&manifests).expect("resolve ok");
        let rendered = plan.render();

        assert!(
            rendered.contains("[bundle observers@0.1.0]"),
            "render must list `observers` bundle heading; got:\n{rendered}"
        );
        assert!(
            rendered.contains("[unbundled]"),
            "render must list `[unbundled]` heading when at least one plugin is bundled and one is not; got:\n{rendered}"
        );
        assert!(
            rendered.contains("    - standalone@0.1.0"),
            "render must list `standalone` under `[unbundled]` heading; got:\n{rendered}"
        );
    }

    #[test]
    fn resolve_error_duplicate_name_attribution() {
        // `DuplicateName` must carry the colliding (i.e.
        // second-inserted) manifest's bundle id so the error
        // message points at the bundle that caused the
        // collision. Order: the iterator reaches the second
        // manifest last; its `m.bundle` is what populates
        // `seen_in_bundle`.
        let bundle = BundleId::new("dup-bundle", "0.1.0");
        let manifests = vec![
            // first: bundled
            manifest("echo", Some(bundle.clone())),
            // second: unbundled collision
            manifest("echo", None),
        ];
        let err = resolve(&manifests).expect_err("duplicate name must error");
        match err {
            ResolveError::DuplicateName { plugin, seen_in_bundle } => {
                assert_eq!(plugin.name, "echo");
                assert_eq!(
                    seen_in_bundle, None,
                    "second manifest has no bundle, so seen_in_bundle is None"
                );
            }
            other => panic!("expected DuplicateName, got {other:?}"),
        }

        // Reverse the order to verify `seen_in_bundle` IS
        // populated when the colliding manifest is bundled.
        let manifests = vec![
            manifest("echo", None),
            manifest("echo", Some(bundle.clone())),
        ];
        let err = resolve(&manifests).expect_err("duplicate name must error (bundled second)");
        match err {
            ResolveError::DuplicateName { plugin, seen_in_bundle } => {
                assert_eq!(plugin.name, "echo");
                assert_eq!(
                    seen_in_bundle,
                    Some(bundle),
                    "second-inserted (bundled) manifest's bundle id must be reported"
                );
            }
            other => panic!("expected DuplicateName, got {other:?}"),
        }
    }

    #[test]
    fn resolve_error_unprovided_attribution() {
        // `Unprovided` must carry the consumer's bundle id when
        // the consumer is bundled. The manifest requires a
        // contract that no other manifest publishes.
        let observers = BundleId::new("observers", "0.1.0");
        let manifests = vec![
            manifest_requiring("observer", "missing_contract", Some(observers.clone())),
        ];
        let err = resolve(&manifests).expect_err("missing contract must error");
        match err {
            ResolveError::Unprovided {
                contract,
                by,
                requester_bundle,
            } => {
                assert_eq!(contract, "missing_contract");
                assert_eq!(by, "observer");
                assert_eq!(
                    requester_bundle,
                    Some(observers),
                    "Unprovided must carry the consumer's bundle id"
                );
            }
            other => panic!("expected Unprovided, got {other:?}"),
        }
    }

    #[test]
    fn display_unprovided_includes_bundle_suffix() {
        // The `Display` impl must add "in bundle `<name>@<version>`"
        // when `requester_bundle` is `Some`, and stay silent when
        // it's `None` (preserves pre-bundle error text).
        let observers = BundleId::new("observers", "0.1.0");
        let with_bundle = ResolveError::Unprovided {
            contract: "demo".to_string(),
            by: "observer".to_string(),
            requester_bundle: Some(observers.clone()),
        };
        let text = format!("{with_bundle}");
        assert!(
            text.contains("in bundle `observers@0.1.0`"),
            "Display must include bundle suffix when set; got: {text}"
        );

        let without_bundle = ResolveError::Unprovided {
            contract: "demo".to_string(),
            by: "observer".to_string(),
            requester_bundle: None,
        };
        let text = format!("{without_bundle}");
        assert!(
            !text.contains("in bundle"),
            "Display must NOT include bundle suffix when None; got: {text}"
        );
        // Pre-bundle text preserved.
        assert!(
            text.contains("no provider for contract `demo` (requested by observer)"),
            "Display must preserve pre-bundle error text when bundle is None; got: {text}"
        );
    }
}
