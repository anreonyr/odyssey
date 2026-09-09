//! Capability Graph — Phase 2 P6: runtime observability of the
//! capability system.
//!
//! The graph is the *static* view: which capabilities are installed
//! in the CSpace, what namespace tree they form, and how an
//! attenuation chain narrows authority. It does not (yet) model
//! plugin-to-plugin dynamic edges; those would need plugin
//! introspection, which Phase 3 may add.
//!
//! ```text
//!                  capability graph
//!
//!                       root
//!                         │
//!          ┌──────────────┼──────────────┐
//!          │              │              │
//!     model.*         db.*           web.*     ← namespace prefix
//!          │              │              │
//!     model.llama3   db.read       web.fetch  ← leaves
//! ```
//!
//! `CapabilityGraph::from(&cspace)` builds a snapshot. The graph is
//! read-only; mutations go through the CSpace.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::capability::{CapabilityMeta, CapabilitySpace};

/// A read-only snapshot of the CSpace's capability layout.
#[derive(Debug, Clone, Serialize)]
pub struct CapabilityGraph {
    pub nodes: Vec<GraphNode>,
    pub namespaces: Vec<NamespaceNode>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphNode {
    pub name: String,
    pub namespace: String,
    pub plugin: String,
    pub streaming: bool,
    pub timeout_ms: u32,
    pub quota_calls_per_minute: u32,
    pub quota_tokens_per_minute: u64,
    pub quota_bytes_per_minute: u64,
}

impl From<&CapabilityMeta> for GraphNode {
    fn from(m: &CapabilityMeta) -> Self {
        GraphNode {
            name: m.name.clone(),
            namespace: m.namespace.clone(),
            plugin: format!("{}@{}", m.plugin.name, m.plugin.version),
            streaming: m.streaming,
            timeout_ms: m.timeout_ms,
            quota_calls_per_minute: m.quota.calls_per_minute,
            quota_tokens_per_minute: m.quota.tokens_per_minute,
            quota_bytes_per_minute: m.quota.bytes_per_minute,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct NamespaceNode {
    pub name: String,
    pub count: usize,
    pub children: Vec<String>,
}

impl CapabilityGraph {
    /// Build a graph from the current state of `cspace`.
    pub fn from(cspace: &CapabilitySpace) -> Self {
        let metas = cspace.enumerate();
        let nodes: Vec<GraphNode> = metas.iter().map(GraphNode::from).collect();
        let namespaces = build_namespace_tree(&metas);
        CapabilityGraph { nodes, namespaces }
    }

    /// Sub-graph filtered by namespace prefix.
    pub fn under(&self, prefix: &str) -> CapabilityGraph {
        let nodes: Vec<GraphNode> = self
            .nodes
            .iter()
            .filter(|n| namespace_starts_with(&n.namespace, prefix))
            .cloned()
            .collect();
        let mut metas: Vec<CapabilityMeta> = Vec::new();
        // Reconstruct metas from nodes for tree build.
        for n in &nodes {
            metas.push(synthesize_meta(n));
        }
        let namespaces = build_namespace_tree(&metas);
        CapabilityGraph { nodes, namespaces }
    }

    /// Pretty-print the namespace tree.
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("capability graph:\n");
        if self.nodes.is_empty() {
            s.push_str("  (empty)\n");
            return s;
        }
        // Group by namespace.
        let mut by_ns: BTreeMap<String, Vec<&GraphNode>> = BTreeMap::new();
        for n in &self.nodes {
            by_ns.entry(n.namespace.clone()).or_default().push(n);
        }
        for (ns, leaves) in &by_ns {
            s.push_str(&format!("  {ns}\n"));
            for l in leaves {
                s.push_str(&format!(
                    "    ├─ {} timeout={}ms streaming={} quota={}c/{}t/{}B\n",
                    l.name, l.timeout_ms, l.streaming,
                    l.quota_calls_per_minute,
                    l.quota_tokens_per_minute,
                    l.quota_bytes_per_minute
                ));
            }
        }
        s
    }
}

fn synthesize_meta(_n: &GraphNode) -> CapabilityMeta {
    // Used only for namespace tree reconstruction; we don't actually
    // need a full CapabilityMeta to compute the tree, so return a
    // placeholder with just the namespace.
    CapabilityMeta {
        id: crate::capability::CapabilityId(0),
        name: String::new(),
        namespace: _n.namespace.clone(),
        plugin: crate::host::manifest::PluginId {
            name: String::new(),
            version: String::new(),
        },
        in_type: String::new(),
        out_type: String::new(),
        streaming: false,
        timeout_ms: 0,
        quota: crate::capability::QuotaSpec::unlimited(),
        contract: crate::capability::CapabilityContract::empty(),
    }
}

fn namespace_starts_with(ns: &str, prefix: &str) -> bool {
    if prefix.is_empty() || prefix == "." {
        return true;
    }
    if ns == prefix {
        return true;
    }
    ns.starts_with(&format!("{prefix}."))
}

fn build_namespace_tree(metas: &[CapabilityMeta]) -> Vec<NamespaceNode> {
    // Group by top-level prefix.
    let mut by_root: BTreeMap<String, Vec<&CapabilityMeta>> = BTreeMap::new();
    for m in metas {
        let root = m.namespace.split('.').next().unwrap_or("").to_string();
        if root.is_empty() {
            continue;
        }
        by_root.entry(root).or_default().push(m);
    }
    by_root
        .into_iter()
        .map(|(root, leaves)| NamespaceNode {
            name: root,
            count: leaves.len(),
            children: leaves.iter().map(|m| m.namespace.clone()).collect(),
        })
        .collect()
}