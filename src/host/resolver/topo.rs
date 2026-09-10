//! Topological sort — Kahn's algorithm with cycle detection.
//!
//! Phase 5: pulled out of `resolver::mod` so the cycle
//! detection logic is testable in isolation. Edges are
//! `provider → consumer` (i.e. "provider must come before
//! consumer in mint order"). The in-degree counter is the
//! number of un-satisfied edges entering each node; nodes
//! with in-degree 0 are roots and emit first.

use std::collections::{BTreeMap, BTreeSet};

use crate::kernel::ids::PluginId;

use super::error::ResolveError;

/// Run Kahn's algorithm over the edge map. Returns the
/// mint order (all nodes with no in-degree first, then
/// consumers whose providers have all been emitted). If
/// the order is shorter than the node count, the remaining
/// nodes form a cycle — surface them as `ResolveError::Cycle`.
pub(crate) fn topological_sort(
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
pub(crate) fn add_edge(
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
