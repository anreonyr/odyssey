//! γ.1 — Capability graph (P6).
//!
//! `CapabilityGraph::from(&cspace)` and `children_of(slot)` expose the
//! parent→child attenuation tree to runtime code.

use odyssey::capability::{CapabilityRights, OperationRights};
use odyssey::plugins::counter::CounterResource;

#[test]
fn graph_walks_attenuation_tree() {
    let (space, factory) = crate::common::boot();
    let root = crate::common::mint_counter(&factory);
    let a = space
        .restrict::<CounterResource>(
            root,
            CapabilityRights {
                operations: OperationRights::READ,
                timeout_ms: crate::common::DEFAULT_TIMEOUT_MS,
            },
            "a".into(),
        )
        .unwrap();
    let b = space
        .restrict::<CounterResource>(
            a,
            CapabilityRights {
                operations: OperationRights::READ,
                timeout_ms: crate::common::DEFAULT_TIMEOUT_MS,
            },
            "b".into(),
        )
        .unwrap();

    let graph = odyssey::capability::graph::CapabilityGraph::from(&space);
    assert_eq!(graph.nodes.len(), 3);
    assert_eq!(space.children_of(root), vec![a]);
    assert_eq!(space.children_of(a), vec![b]);
    assert_eq!(space.children_of(b), Vec::<odyssey::capability::SlotId>::new());
    assert_eq!(space.enumerate_namespace("").len(), 3);
}