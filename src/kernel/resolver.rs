//! Capability resolver — Phase 3 P3.1.
//!
//! Turns a flat list of [`PluginManifest`]s into a [`ResolvedPlan`]:
//! the topological order in which plugins must mint their
//! capabilities, and the per-plugin binding table that tells
//! each plugin which [`crate::capability::SlotId`] fulfils each
//! `[[requires]] contract`.
//!
//! ## Algorithm
//!
//! 1. **Contract index.** Walk every manifest's `[[exposes]]`,
//!    index each non-empty `contract_name` to its provider
//!    (`PluginId`, capability name).
//! 2. **Edges.** Walk every manifest's `[[requires]]`. For each
//!    requirement:
//!      - if no provider: `ResolveError::Unprovided`
//!      - if >1 providers: `ResolveError::Ambiguous`
//!      - else: add an edge `provider → consumer` and record a
//!        [`ResolvedBinding`] for the consumer.
//! 3. **Topological sort.** Kahn's algorithm on the edge graph.
//!    Nodes with in-degree zero (no remaining dependencies) are
//!    dequeued first. Their outgoing edges decrement the
//!    downstream node's in-degree. If anything is left with
//!    non-zero in-degree at the end, the leftover forms a cycle
//!    and we return [`ResolveError::Cycle`].
//!
//! Plugins with no `requires` are pure providers and have
//! in-degree zero; they sort to the front of `mint_order` in
//! alphabetical order (stable, deterministic output for tests).
//!
//! ## Why contract-keyed, not plugin-keyed
//!
//! The Phase 2 `[[consumes]]` declared `plugin + version +
//! capability`. That made plugin upgrades break dependents even
//! when the capability shape was unchanged. Phase 3 P3.1 keys
//! dependencies on the **contract name** instead. As long as the
//! provider's `[[exposes]] contract_name` stays the same, any
//! version of any plugin can fulfil the requirement. Version
//! stability is opt-in (a future `[[requires]] version = "..."`
//! field); the default is contract-stability.

use std::collections::{BTreeMap, BTreeSet};

use crate::kernel::manifest::{PluginId, PluginManifest};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    /// A `[[requires]]` block names a contract that no loaded
    /// manifest publishes.
    #[error(
        "contract '{contract}' required by {by} has no provider (no loaded manifest publishes it)"
    )]
    Unprovided { contract: String, by: String },

    /// A contract has multiple providers and the resolver has no
    /// way to pick one. Future: priority hints or version ranges.
    #[error("contract '{contract}' is ambiguous: {a} and {b} both provide it")]
    Ambiguous {
        contract: String,
        a: String,
        b: String,
    },

    /// The dependency graph contains a cycle. The `chain` lists
    /// the plugins still stuck with non-zero in-degree when the
    /// sort ran out — every plugin in there is part of (or
    /// downstream of) the cycle.
    #[error("dependency cycle involving: {chain:?}")]
    Cycle { chain: Vec<String> },
}

// ---------------------------------------------------------------------------
// Plan
// ---------------------------------------------------------------------------

/// One capability binding delivered to a plugin at boot time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedBinding {
    /// Local handle inside the consuming plugin. The plugin code
    /// uses this to look up its received slot reference, e.g.
    /// `ctx.require("slot:embedder")`.
    pub handle: String,
    /// The plugin that will mint the capability that fulfils
    /// this binding.
    pub provider: PluginId,
    /// The capability name within the provider's `[[exposes]]`
    /// (i.e. the cap's name, not its contract name).
    pub capability: String,
    /// The contract name that matched. Echoed back for
    /// diagnostics; identical to the `[[requires]] contract`
    /// field the consumer declared.
    pub contract: String,
}

/// The full resolved boot plan: mint order + per-plugin
/// bindings. Consuming code (the boot sequence) iterates
/// `mint_order` and, for each plugin, looks up `bindings` to
/// wire the received slots into the plugin's cordis context.
#[derive(Debug, Clone)]
pub struct ResolvedPlan {
    /// Plugins in topological order — every provider in this list
    /// appears before every consumer that depends on it.
    pub mint_order: Vec<PluginId>,
    /// For each plugin in `mint_order`, the bindings it
    /// receives. Plugins with no `requires` get an empty (but
    /// present) entry, so consumers don't need to special-case
    /// missing keys.
    pub bindings: BTreeMap<PluginId, Vec<ResolvedBinding>>,
}

// ---------------------------------------------------------------------------
// Resolve
// ---------------------------------------------------------------------------

/// Resolve a set of manifests into a mint plan. Returns
/// [`ResolveError`] on missing providers, ambiguous contracts, or
/// dependency cycles.
pub fn resolve(manifests: &[PluginManifest]) -> Result<ResolvedPlan, ResolveError> {
    // 1. Contract index. A contract may have multiple providers
    //    in principle (e.g. an embedder + a cache-embedder both
    //    publishing "embedder"); for P3.1 we treat that as an
    //    error rather than picking arbitrarily. Future phases may
    //    add a `[[provides]] priority` field.
    let mut by_contract: BTreeMap<String, Vec<(PluginId, String)>> = BTreeMap::new();
    for m in manifests {
        for cap in &m.exposes {
            if cap.contract_name.is_empty() {
                continue;
            }
            by_contract
                .entry(cap.contract_name.clone())
                .or_default()
                .push((m.plugin.clone(), cap.name.clone()));
        }
    }

    // 2. Walk `requires`, build edges, build binding table.
    //
    //    Edge direction: provider → consumer. In Kahn's terms,
    //    the provider is a dependency of the consumer (consumer
    //    cannot mint until provider has minted). Topological
    //    sort: nodes with no incoming edges (i.e. nothing
    //    depends on them) come out last; we want the opposite,
    //    so we sort in reverse, OR use the standard formulation
    //    with edges drawn from dep → consumer and "in-degree"
    //    meaning "number of unmet deps".
    //
    //    Here we use the standard formulation directly: in-degree
    //    = number of providers a plugin needs. Leaves of the
    //    graph (in-degree 0) come out first.
    let mut deps: BTreeMap<PluginId, BTreeSet<PluginId>> = BTreeMap::new();
    let mut bindings: BTreeMap<PluginId, Vec<ResolvedBinding>> = BTreeMap::new();

    for m in manifests {
        deps.entry(m.plugin.clone()).or_default();
        bindings.entry(m.plugin.clone()).or_default();
        for req in &m.requires {
            let providers = by_contract.get(&req.contract).ok_or_else(|| {
                ResolveError::Unprovided {
                    contract: req.contract.clone(),
                    by: m.plugin.name.clone(),
                }
            })?;
            if providers.is_empty() {
                return Err(ResolveError::Unprovided {
                    contract: req.contract.clone(),
                    by: m.plugin.name.clone(),
                });
            }
            if providers.len() > 1 {
                // Strict for P3.1 — no priority hints yet, so
                // even two providers are fatal. The error lists
                // the first two so users can see the conflict.
                let (a, _) = &providers[0];
                let (b, _) = &providers[1];
                return Err(ResolveError::Ambiguous {
                    contract: req.contract.clone(),
                    a: format!("{}@{}", a.name, a.version),
                    b: format!("{}@{}", b.name, b.version),
                });
            }
            let (provider, cap_name) = &providers[0];
            deps.entry(m.plugin.clone())
                .or_default()
                .insert(provider.clone());
            bindings
                .entry(m.plugin.clone())
                .or_default()
                .push(ResolvedBinding {
                    handle: req.name.clone(),
                    provider: provider.clone(),
                    capability: cap_name.clone(),
                    contract: req.contract.clone(),
                });
        }
    }

    // 3. Kahn's algorithm with cycle detection.
    //
    //    in_degree[A] = number of providers A requires.
    //    Initial queue = plugins with no requires (leaves of the
    //    dependency graph = pure providers).
    let mut in_degree: BTreeMap<PluginId, usize> = BTreeMap::new();
    for (plugin, dep_set) in &deps {
        in_degree.insert(plugin.clone(), dep_set.len());
    }
    // Reverse edges for decrement step: provider → [consumers].
    let mut consumers_of: BTreeMap<PluginId, Vec<PluginId>> = BTreeMap::new();
    for (plugin, dep_set) in &deps {
        for d in dep_set {
            consumers_of.entry(d.clone()).or_default().push(plugin.clone());
        }
    }

    // Stable, deterministic initial ordering: alphabetical by
    // plugin name, then version. Plugins with the same name but
    // different versions are treated as distinct nodes (the
    // PluginId carries version).
    //
    // We use VecDeque + pop_front so newly-unblocked consumers
    // are appended in alphabetical order rather than processed
    // LIFO from a Vec.
    let mut queue: std::collections::VecDeque<PluginId> = in_degree
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(p, _)| p.clone())
        .collect();
    // in_degree is a BTreeMap so iteration is already sorted
    // ascending; the VecDeque order matches that.

    let mut mint_order = Vec::with_capacity(manifests.len());
    while let Some(p) = queue.pop_front() {
        mint_order.push(p.clone());
        if let Some(consumers) = consumers_of.get(&p) {
            // Sorted for determinism — without this the output
            // of `mint_order` could vary across runs if the
            // BTreeMap iteration order is unstable.
            let mut consumers = consumers.clone();
            consumers.sort_by(|a, b| (&a.name, &a.version).cmp(&(&b.name, &b.version)));
            for c in consumers {
                if let Some(d) = in_degree.get_mut(&c) {
                    *d = d.saturating_sub(1);
                    if *d == 0 {
                        queue.push_back(c.clone());
                    }
                }
            }
        }
    }

    if mint_order.len() != manifests.len() {
        // Anything still with non-zero in-degree is part of (or
        // downstream of) a cycle. We surface all of them so the
        // error tells the user the full extent of the cycle
        // rather than just one link.
        let remaining: Vec<String> = in_degree
            .iter()
            .filter(|(_, d)| **d > 0)
            .map(|(p, _)| format!("{}@{}", p.name, p.version))
            .collect();
        return Err(ResolveError::Cycle { chain: remaining });
    }

    Ok(ResolvedPlan {
        mint_order,
        bindings,
    })
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

impl ResolvedPlan {
    /// Pretty-print the plan. Used by `boot.rs` to log the
    /// resolved mint order + binding table at startup so
    /// operators can see what got wired up.
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("resolved plan:\n");
        s.push_str("  mint order:\n");
        for (i, p) in self.mint_order.iter().enumerate() {
            s.push_str(&format!(
                "    {:>2}. {}@{}\n",
                i + 1,
                p.name,
                p.version
            ));
        }
        s.push_str("  bindings:\n");
        for plugin in &self.mint_order {
            match self.bindings.get(plugin) {
                Some(bs) if !bs.is_empty() => {
                    s.push_str(&format!("    {}@{} receives:\n", plugin.name, plugin.version));
                    for b in bs {
                        s.push_str(&format!(
                            "      - handle={}  contract={}  from={}@{} (cap={})\n",
                            b.handle, b.contract, b.provider.name, b.provider.version, b.capability
                        ));
                    }
                }
                _ => {
                    s.push_str(&format!(
                        "    {}@{}  (no bindings)\n",
                        plugin.name, plugin.version
                    ));
                }
            }
        }
        s
    }
}

// ---------------------------------------------------------------------------
// Tests — unit tests over the resolver algorithm itself.
// Integration tests (resolver + factory + cspace) live in
// tests/epsilon/p3_1_injection.rs.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::manifest::{
        CapabilityDecl, CapabilityRequirement, IsolationMode, PluginId, ResourceHints,
    };

    fn pid(name: &str, version: &str) -> PluginId {
        PluginId {
            name: name.into(),
            version: version.into(),
        }
    }

    fn exposes(contract: &str) -> CapabilityDecl {
        CapabilityDecl {
            name: contract.into(),
            in_type: "any".into(),
            out_type: "any".into(),
            streaming: false,
            contract_name: contract.into(),
            authority: Default::default(),
            protocol: Default::default(),
        }
    }

    fn empty_exposes(name: &str) -> CapabilityDecl {
        CapabilityDecl {
            name: name.into(),
            in_type: "any".into(),
            out_type: "any".into(),
            streaming: false,
            // no contract_name — direct slot lookup only
            contract_name: String::new(),
            authority: Default::default(),
            protocol: Default::default(),
        }
    }

    fn requires(handle: &str, contract: &str) -> CapabilityRequirement {
        CapabilityRequirement {
            name: handle.into(),
            contract: contract.into(),
        }
    }

    fn manifest(
        name: &str,
        version: &str,
        exp: Vec<CapabilityDecl>,
        req: Vec<CapabilityRequirement>,
    ) -> PluginManifest {
        PluginManifest {
            plugin: pid(name, version),
            isolate: IsolationMode::InProc,
            exposes: exp,
            requires: req,
            consumes: Vec::new(),
            host: Vec::new(),
            resources: ResourceHints::default(),
        }
    }

    #[test]
    fn single_provider_no_deps_is_alone() {
        let ms = vec![manifest("gen", "0.1.0", vec![exposes("generator")], vec![])];
        let plan = resolve(&ms).unwrap();
        assert_eq!(plan.mint_order.len(), 1);
        assert_eq!(plan.mint_order[0].name, "gen");
        assert!(plan.bindings.get(&pid("gen", "0.1.0")).unwrap().is_empty());
    }

    #[test]
    fn provider_comes_before_consumer() {
        let ms = vec![
            manifest("agent", "0.1.0", vec![exposes("agent")], vec![requires("gen", "generator")]),
            manifest("gen", "0.1.0", vec![exposes("generator")], vec![]),
        ];
        let plan = resolve(&ms).unwrap();
        let names: Vec<&str> = plan.mint_order.iter().map(|p| p.name.as_str()).collect();
        let pos = |n: &str| names.iter().position(|x| *x == n).unwrap();
        assert!(pos("gen") < pos("agent"));
    }

    #[test]
    fn unprovided_contract_fails() {
        let ms = vec![manifest(
            "agent",
            "0.1.0",
            vec![exposes("agent")],
            vec![requires("gen", "ghost")],
        )];
        let err = resolve(&ms).unwrap_err();
        assert!(matches!(err, ResolveError::Unprovided { .. }));
    }

    #[test]
    fn ambiguous_contract_fails() {
        let ms = vec![
            manifest("a", "0.1.0", vec![exposes("shared")], vec![]),
            manifest("b", "0.1.0", vec![exposes("shared")], vec![]),
            manifest(
                "user",
                "0.1.0",
                vec![exposes("user")],
                vec![requires("s", "shared")],
            ),
        ];
        let err = resolve(&ms).unwrap_err();
        assert!(matches!(err, ResolveError::Ambiguous { .. }));
    }

    #[test]
    fn cycle_detection() {
        // a requires b; b requires a.
        let ms = vec![
            manifest(
                "a",
                "0.1.0",
                vec![exposes("a_contract")],
                vec![requires("b_handle", "b_contract")],
            ),
            manifest(
                "b",
                "0.1.0",
                vec![exposes("b_contract")],
                vec![requires("a_handle", "a_contract")],
            ),
        ];
        let err = resolve(&ms).unwrap_err();
        match err {
            ResolveError::Cycle { chain } => {
                assert_eq!(chain.len(), 2);
                assert!(chain.iter().any(|s| s.starts_with("a@")));
                assert!(chain.iter().any(|s| s.starts_with("b@")));
            }
            other => panic!("expected Cycle, got {other:?}"),
        }
    }

    #[test]
    fn diamond_dependency_orders_correctly() {
        // a is a shared provider.
        // b and c both require a.
        // d requires both b and c.
        let ms = vec![
            manifest("a", "0.1.0", vec![exposes("a_contract")], vec![]),
            manifest(
                "b",
                "0.1.0",
                vec![exposes("b_contract")],
                vec![requires("a", "a_contract")],
            ),
            manifest(
                "c",
                "0.1.0",
                vec![exposes("c_contract")],
                vec![requires("a", "a_contract")],
            ),
            manifest(
                "d",
                "0.1.0",
                vec![exposes("d_contract")],
                vec![
                    requires("b", "b_contract"),
                    requires("c", "c_contract"),
                ],
            ),
        ];
        let plan = resolve(&ms).unwrap();
        let pos = |n: &str| {
            plan.mint_order
                .iter()
                .position(|p| p.name == n)
                .expect("plugin present in plan")
        };
        assert!(pos("a") < pos("b"));
        assert!(pos("a") < pos("c"));
        assert!(pos("b") < pos("d"));
        assert!(pos("c") < pos("d"));
        // d receives two bindings
        let db = plan.bindings.get(&pid("d", "0.1.0")).unwrap();
        assert_eq!(db.len(), 2);
        let handles: Vec<&str> = db.iter().map(|b| b.handle.as_str()).collect();
        assert!(handles.contains(&"b"));
        assert!(handles.contains(&"c"));
    }

    #[test]
    fn plugins_with_no_requires_sort_alphabetically() {
        let ms = vec![
            manifest("zebra", "0.1.0", vec![empty_exposes("z")], vec![]),
            manifest("alpha", "0.1.0", vec![empty_exposes("a")], vec![]),
            manifest("mango", "0.1.0", vec![empty_exposes("m")], vec![]),
        ];
        let plan = resolve(&ms).unwrap();
        let names: Vec<&str> = plan.mint_order.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "mango", "zebra"]);
    }

    #[test]
    fn caps_without_contract_name_are_unreachable_via_injection() {
        // The provider publishes no contract_name; a consumer
        // asking for that contract must fail with Unprovided.
        let ms = vec![
            manifest("gen", "0.1.0", vec![empty_exposes("generate")], vec![]),
            manifest(
                "user",
                "0.1.0",
                vec![exposes("user")],
                vec![requires("g", "generate")],
            ),
        ];
        let err = resolve(&ms).unwrap_err();
        assert!(matches!(err, ResolveError::Unprovided { .. }));
    }

    #[test]
    fn self_dependency_is_a_cycle() {
        // a requires a contract that a itself provides. Self-loop.
        let ms = vec![manifest(
            "self",
            "0.1.0",
            vec![exposes("self_contract")],
            vec![requires("self", "self_contract")],
        )];
        let err = resolve(&ms).unwrap_err();
        assert!(matches!(err, ResolveError::Cycle { .. }));
    }
}
