//! Snapshot view of the cspace — a read-only, JSON-serialisable
//! graph for HTTP bridge / introspection. Phase 5: this is the only
//! graph view; the Phase 4 `capability::graph` is removed.
//!
//! `GraphNode` and `CapabilityNode` are unified: the public graph
//! type carries one node shape (`GraphNode`), and `CapabilityNode`
//! is kept as a type alias so existing call sites compile.
//! `CapabilityGraph::capabilities` is renamed to `nodes` to match
//! the public contract (`graph.nodes.len()` etc.).

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
#[derive(Debug, Clone, Serialize)]
pub struct NamespaceNode {
    pub namespace: String,
    pub children: Vec<NamespaceNode>,
    pub capabilities: Vec<GraphNode>,
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

        // Build namespace tree from the same metas.
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
    let mut root: BTreeMap<String, (Vec<NamespaceNode>, Vec<GraphNode>)> = BTreeMap::new();
    for m in metas {
        let segments: Vec<&str> = if m.namespace.is_empty() {
            vec![""]
        } else {
            m.namespace.split('.').collect()
        };
        // Flattened single-level representation: each capability is
        // filed under its first segment. Recursion into deeper
        // segments is deferred to Phase 6 if needed.
        let head = segments.first().copied().unwrap_or("").to_string();
        let node = GraphNode {
            // Slot id is unknown at the namespace-tree level
            // (we only have meta here, no slot→meta index).
            // The graph's flat `nodes` field carries the real
            // ids; the tree view uses a sentinel of slot 1,
            // which the HTTP bridge substitutes with the real
            // id when it joins the two views.
            slot: SlotId::new(1),
            capability_id: m.id.clone(),
            name: m.name.clone(),
            namespace: m.namespace.clone(),
            contract_name: m.contract_name.clone(),
            plugin: m.plugin.clone(),
            operations: format!("{:?}", m.authority),
            timeout_ms: m.timeout_ms,
            parent: None,
            quota_calls_per_minute: m.quota.calls_per_minute,
        };
        root.entry(head).or_default().1.push(node);
    }
    root.into_iter()
        .map(|(ns, (children, caps))| NamespaceNode {
            namespace: ns,
            children,
            capabilities: caps,
        })
        .collect()
}

/// Back-compat alias for the previous Phase 4 `CapabilityNode`.
/// `GraphNode` is the canonical name now; existing code that
/// referenced `CapabilityNode` keeps compiling through this alias.
pub type CapabilityNode = GraphNode;
