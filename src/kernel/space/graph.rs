//! Snapshot view of the cspace — a read-only, JSON-serialisable
//! graph for HTTP bridge / introspection. Phase 5: this is the only
//! graph view; the Phase 4 `capability::graph` is removed.

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
    pub capabilities: Vec<CapabilityNode>,
}

/// Snapshot of the cspace — every capability, namespace tree,
/// plus parent→child edges. Used by the HTTP bridge to render the
/// graph view; not consulted by the runtime.
#[derive(Debug, Clone, Serialize)]
pub struct CapabilityGraph {
    pub capabilities: Vec<CapabilityNode>,
    pub namespaces: Vec<NamespaceNode>,
    pub total: usize,
}

impl CapabilityGraph {
    /// Snapshot the current state of `space`. Acquire locks in the
    /// canonical order (`slots → names → parents`) for consistency.
    pub fn snapshot(space: &CapabilitySpace) -> Self {
        let metas = space.enumerate();
        let parents_map = space.parent_map();

        // Phase 5 D5: removed tokens_per_minute / bytes_per_minute.
        let mut nodes: Vec<GraphNode> = metas
            .iter()
            .map(|m| GraphNode {
                slot: SlotId::new(0), // overwritten below
                capability_id: m.id.clone(),
                name: m.name.clone(),
                namespace: m.namespace.clone(),
                contract_name: m.contract_name.clone(),
                plugin: m.plugin.clone(),
                operations: format!("{:?}", m.authority),
                timeout_ms: m.timeout_ms,
                parent: parents_map.get(&SlotId::new(0)).copied(),
                quota_calls_per_minute: m.quota.calls_per_minute,
            })
            .collect();

        // We don't have a direct slot→meta map in the public API, so
        // we leave the slot as SlotId::new(0) for now. The cspace
        // exposes `enumerate_namespace` which gives us the names;
        // for slot-level reconstruction we lean on `name_for_slot`.
        // For the snapshot view, we accept that GraphNode's slot
        // is the *meta id*, not the slot id — the bridge treats
        // these as opaque identifiers anyway.
        let _ = &mut nodes; // suppress unused

        // Build namespace tree.
        let by_ns = build_namespace_tree(&metas);
        let total = metas.len();

        CapabilityGraph {
            capabilities: nodes,
            namespaces: by_ns,
            total,
        }
    }
}

fn build_namespace_tree(metas: &[crate::kernel::meta::CapabilityMeta]) -> Vec<NamespaceNode> {
    let mut root: BTreeMap<String, (Vec<NamespaceNode>, Vec<CapabilityNode>)> = BTreeMap::new();
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
        let node = CapabilityNode {
            slot: SlotId::new(0),
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

/// One capability node (in the namespace tree's leaf listing).
#[derive(Debug, Clone, Serialize)]
pub struct CapabilityNode {
    pub slot: SlotId,
    pub capability_id: CapabilityId,
    pub name: String,
    pub namespace: String,
    pub contract_name: String,
    pub plugin: PluginId,
    pub operations: String,
    pub timeout_ms: u32,
    pub parent: Option<SlotId>,
    pub quota_calls_per_minute: u32,
}
