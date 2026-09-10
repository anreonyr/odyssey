//! Snapshot view of the cspace — a read-only, JSON-serialisable
//! graph for HTTP bridge / introspection. Phase 5: this is the only
//! graph view; the Phase 4 `capability::graph` is removed.
//!
//! `GraphNode` and `CapabilityNode` are unified: the public graph
//! type carries one node shape (`GraphNode`), and `CapabilityNode`
//! is kept as a type alias so existing call sites compile.
//! `CapabilityGraph::capabilities` is renamed to `nodes` to match
//! the public contract (`graph.nodes.len()` etc.).
//!
//! `NamespaceNode` carries only the namespace tree
//! (`namespace` + `children`). The flat `nodes` field on
//! `CapabilityGraph` is the single source of truth for
//! capability nodes — Phase 6 dropped the duplicated
//! `Vec<GraphNode>` per-namespace field because it could not
//! be populated with real `SlotId`s at the namespace level
//! (the namespace builder only has `CapabilityMeta` in hand,
//! not a `SlotId`).

use std::collections::BTreeMap;

use serde::Serialize;

use super::CapabilitySpace;
use crate::kernel::ids::{CapabilityId, PluginId, SlotId};

/// One capability node in the graph view.
#[derive(Debug, Clone, Serialize)]
pub struct GraphNode {
    pub slot: SlotId,
    pub capability_id: CapabilityId,
    pub name: String,
    pub namespace: String,
    pub contract_name: String,
    pub plugin: PluginId,
    pub operations: String,
    pub timeout_ms: u32,
    pub parent: Option<SlotId>,
    /// Phase 5 D5: `quota_calls_per_minute` only — `tokens`/`bytes`
    /// fields removed because the kernel no longer tracks them.
    pub quota_calls_per_minute: u32,
}

/// One namespace node in the graph view.
///
/// Phase 6: `capabilities` field removed. The namespace tree
/// only carries the structural shape (`namespace` + nested
/// `children`); the flat `CapabilityGraph::nodes` array is the
/// single source of truth for every capability. Consumers
/// (HTTP bridge, observability tooling) walk `nodes` once and
/// join by `namespace` string when they need a per-namespace
/// grouping. Carrying the same `Vec<GraphNode>` per-namespace
/// forced the builder to invent a `SlotId::new(1)` sentinel
/// because the builder only sees `CapabilityMeta`, not the
/// underlying slot id.
#[derive(Debug, Clone, Serialize)]
pub struct NamespaceNode {
    pub namespace: String,
    pub children: Vec<NamespaceNode>,
}

/// Snapshot of the cspace — every capability, namespace tree,
/// plus parent→child edges. Used by the HTTP bridge to render the
/// graph view; not consulted by the runtime.
#[derive(Debug, Clone, Serialize)]
pub struct CapabilityGraph {
    pub nodes: Vec<GraphNode>,
    pub namespaces: Vec<NamespaceNode>,
    pub total: usize,
}

impl CapabilityGraph {
    /// Snapshot the current state of `space`. Acquire locks in the
    /// canonical order (`slots → names → parents`) for consistency.
    pub fn snapshot(space: &CapabilitySpace) -> Self {
        Self::from(space)
    }
}

impl From<&CapabilitySpace> for CapabilityGraph {
    fn from(space: &CapabilitySpace) -> Self {
        // Snapshot every slot along with its canonical name
        // and meta. This is the only path that can produce a
        // real `SlotId` per node — `enumerate()` only gives us
        // names.
        let index = space.snapshot_index();
        let parents_map = space.parent_map();

        // Phase 5 D5: removed tokens_per_minute / bytes_per_minute.
        let nodes: Vec<GraphNode> = index
            .iter()
            .map(|(slot_id, m)| GraphNode {
                slot: *slot_id,
                capability_id: m.id.clone(),
                name: m.name.clone(),
                namespace: m.namespace.clone(),
                contract_name: m.contract_name.clone(),
                plugin: m.plugin.clone(),
                operations: format!("{:?}", m.authority),
                timeout_ms: m.timeout_ms,
                parent: parents_map.get(slot_id).copied(),
                quota_calls_per_minute: m.quota.calls_per_minute,
            })
            .collect();

        // Build the namespace tree from the metas' namespace
        // strings. The tree carries no capability nodes —
        // consumers walk `nodes` and join by `namespace` when
        // they need a per-namespace grouping.
        let metas: Vec<crate::kernel::meta::CapabilityMeta> =
            index.into_iter().map(|(_, m)| m).collect();
        let namespaces = build_namespace_tree(&metas);
        let total = metas.len();

        CapabilityGraph {
            nodes,
            namespaces,
            total,
        }
    }
}

fn build_namespace_tree(metas: &[crate::kernel::meta::CapabilityMeta]) -> Vec<NamespaceNode> {
    let mut root: BTreeMap<String, Vec<NamespaceNode>> = BTreeMap::new();
    for m in metas {
        let segments: Vec<&str> = if m.namespace.is_empty() {
            vec![""]
        } else {
            m.namespace.split('.').collect()
        };
        // Flattened single-level representation: each namespace is
        // filed under its first segment. Recursion into deeper
        // segments is deferred to Phase 6 if needed.
        let head = segments.first().copied().unwrap_or("").to_string();
        // Each namespace appears once in the tree; deeper
        // segments live under `children` but the current
        // builder does not synthesise them. Phase 6 callers
        // that need deeper trees can call `enumerate_namespace`
        // directly on the cspace.
        root.entry(head).or_default();
    }
    root.into_iter()
        .map(|(ns, children)| NamespaceNode {
            namespace: ns,
            children,
        })
        .collect()
}

/// Back-compat alias for the previous Phase 4 `CapabilityNode`.
/// `GraphNode` is the canonical name now; existing code that
/// referenced `CapabilityNode` keeps compiling through this alias.
pub type CapabilityNode = GraphNode;
