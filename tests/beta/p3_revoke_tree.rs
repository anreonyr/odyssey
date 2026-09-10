//! β.2 — Multi-hop revocation (extends P3/P4).
//!
//! Revoking an intermediate hop (`revoke_tree`) severs every
//! descendant.

use odyssey::kernel::{CapabilityRights, OperationRights, Slot};
use odyssey::plugins::test_only::counter::CounterResource;
use serde_json::json;

#[test]
fn descendants_killed_by_revoke_tree() {
    let (space, factory) = crate::common::boot();
    let root = crate::common::mint_counter(&factory);

    // root → a → b → c
    let a = space
        .restrict::<CounterResource>(
            root,
            CapabilityRights {
                operations: OperationRights::READ | OperationRights::WRITE,
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
    let c = space
        .restrict::<CounterResource>(
            b,
            CapabilityRights {
                operations: OperationRights::READ,
                timeout_ms: crate::common::DEFAULT_TIMEOUT_MS,
            },
            "c".into(),
        )
        .unwrap();

    // Pre-revoke, c works.
    assert!(Slot::<CounterResource>::new(space.clone(), c)
        .invoke_op(OperationRights::READ, json!({"op": "read"}))
        .is_ok());

    // Revoke b's subtree.
    let removed = space.revoke_tree(b);
    assert_eq!(removed, 2, "expected b + c removed, got {removed}");

    // a still works; b and c are gone.
    assert!(Slot::<CounterResource>::new(space.clone(), a)
        .invoke_op(OperationRights::READ, json!({"op": "read"}))
        .is_ok());
    assert!(Slot::<CounterResource>::new(space.clone(), b).capability().is_none());
    assert!(Slot::<CounterResource>::new(space.clone(), c).capability().is_none());
}