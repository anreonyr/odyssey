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

use crate::capability::CapabilitySpace;

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

impl GraphNode {
    /// Build a `GraphNode` directly from the parts the tree builder
    /// needs, without going through `CapabilityMeta`. Used by both
    /// `from(&cspace)` (which has full metas) and `under(prefix)`
    /// (which only has nodes).
    fn view(&self) -> (&str, &str) {
        (self.namespace.as_str(), self.name.as_str())
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
        let nodes: Vec<GraphNode> = metas
            .iter()
            .map(|m| GraphNode {
                name: m.name.clone(),
                namespace: m.namespace.clone(),
                plugin: format!("{}@{}", m.plugin.name, m.plugin.version),
                streaming: m.streaming,
                timeout_ms: m.timeout_ms,
                quota_calls_per_minute: m.quota.calls_per_minute,
                quota_tokens_per_minute: m.quota.tokens_per_minute,
                quota_bytes_per_minute: m.quota.bytes_per_minute,
            })
            .collect();
        let namespaces = build_namespace_tree(nodes.iter().map(GraphNode::view));
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
        let namespaces = build_namespace_tree(nodes.iter().map(GraphNode::view));
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

fn namespace_starts_with(ns: &str, prefix: &str) -> bool {
    if prefix.is_empty() || prefix == "." {
        return true;
    }
    if ns == prefix {
        return true;
    }
    ns.starts_with(&format!("{prefix}."))
}

/// Group namespaces by their top-level segment. The tree builder only
/// needs the (namespace, name) pair — full `CapabilityMeta` is unused
/// inside this function.
fn build_namespace_tree<'a, I>(leaves: I) -> Vec<NamespaceNode>
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    // Group by top-level prefix.
    let mut by_root: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for (namespace, name) in leaves {
        let root = namespace.split('.').next().unwrap_or("").to_string();
        if root.is_empty() {
            continue;
        }
        by_root
            .entry(root)
            .or_default()
            .push((namespace.to_string(), name.to_string()));
    }
    by_root
        .into_iter()
        .map(|(root, leaves)| NamespaceNode {
            name: root,
            count: leaves.len(),
            children: leaves.into_iter().map(|(ns, _)| ns).collect(),
        })
        .collect()
}