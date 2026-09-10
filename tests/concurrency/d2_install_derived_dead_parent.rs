//! D2 reproducer: `install_derived` can commit a dispatchable cap whose
//! parent slot has already been removed by a concurrent `revoke_tree`.
//!
//! This test runs the *exact* race window from the council Pass 2
//! walk-through. Two paths are exercised:
//!
//! - **Test 1 — install_after_revoke_tree_refuses_dead_parent**:
//!   pre-revoke the parent, then grant. The grant must fail because
//!   the parent no longer exists.
//!
//! - **Test 2 — concurrent_grant_during_revoke_tree**: many threads
//!   race; the post-condition is that no surviving non-root cap has
//!   a dead parent pointer. We approximate this by asserting that
//!   `enumerate()` never returns a non-root cap after the parent's
//!   revoke_tree completes (because every grant parented at the
//!   root must either be swept by the sweep or be refused at the
//!   install_derived precondition check).

use std::sync::Arc;

use odyssey::capability::{
    Capability, CapabilityBudget, CapabilityRights, CapabilitySpace, CapKind, OperationRights,
    SlotId,
};
use odyssey::kernel::factory::CapabilityFactory;
use odyssey::kernel::manifest::{CapabilityDecl, PluginId};
use odyssey::plugins::test_only::counter::CounterResource;

/// Test 1 — sequential: revoke the parent first, then try to grant.
/// On pre-Phase-5 code, the grant may succeed (install_derived does
/// not check that the parent is still in `parents`). On Phase-5 code,
/// the grant MUST fail with `CapabilityError::SlotEmpty`.
#[test]
fn grant_after_revoke_tree_refuses_dead_parent() {
    let (space, factory) = boot();

    let root = mint_counter(&factory, "root");
    let _typed_root: Arc<Capability<CounterResource>> = space
        .lookup_typed::<CounterResource>(root)
        .expect("root typed cap");

    // Remove the parent.
    let removed = space.revoke_tree(root);
    assert!(removed >= 1, "revoke_tree should remove at least the root");

    // Now try to grant a child of the dead root.
    let grant_result = space.grant::<CounterResource>(
        root,
        CapabilityRights {
            operations: OperationRights::READ,
            timeout_ms: 5000,
        },
        "after_revoke".into(),
    );

    match grant_result {
        Err(odyssey::capability::CapabilityError::SlotEmpty(_)) => {
            // Phase-5 code: parent-live check fires.
        }
        Ok(new_slot) => {
            // Pre-Phase-5 code: install succeeded.
            // Verify the orphan is dispatchable through lookup_by_name.
            let dispatchable = space.lookup_by_name("after_revoke").is_some();
            assert!(
                !dispatchable,
                "INTERLEAVING 2 REPRODUCED: orphan cap installed at slot {} with parent {} (deleted); revoked=false",
                new_slot.raw(),
                root.raw()
            );
        }
        Err(e) => panic!("unexpected error variant: {e:?}"),
    }

    // Sanity: the root must be gone.
    assert!(
        space.lookup_by_name("root").is_none(),
        "root must be revoked"
    );
}

/// Test 2 — concurrent stress: many grant/revoke_tree threads; final
/// invariant is that `enumerate()` never reports a non-root cap whose
/// parent chain doesn't reach a root.
///
/// We approximate the invariant by asserting that after every thread
/// quiesces AND a final `revoke_tree(root)` is run, `enumerate()`
/// returns at most one entry (the root if it survived, else empty).
///
/// On pre-Phase-5 code: a concurrent grant can land AFTER
/// `revoke_with_sweep` releases its locks, producing a cap with a
/// parent pointer to the now-deleted root. That cap survives the
/// final revoke_tree because its parent's id is no longer in
/// `parents`. `enumerate()` returns more than one entry.
///
/// On Phase-5 code: the parent-live check in `install_derived` fires
/// before the lock is acquired, so the install is refused. After the
/// final revoke_tree, only the root may remain (or nothing).
#[test]
fn concurrent_grant_and_revoke_tree_yields_no_orphans() {
    let (space, factory) = boot();

    let root = mint_counter(&factory, "root");
    let _typed_root: Arc<Capability<CounterResource>> = space
        .lookup_typed::<CounterResource>(root)
        .expect("root typed cap");

    let rights = CapabilityRights {
        operations: OperationRights::READ,
        timeout_ms: 5000,
    };

    // Spawn workers that alternate grant and revoke_tree.
    let handles: Vec<_> = (0..32)
        .map(|i| {
            let space = space.clone();
            std::thread::spawn(move || {
                if i % 2 == 0 {
                    let _ = space.grant::<CounterResource>(
                        root,
                        rights,
                        format!("child_{i}"),
                    );
                } else {
                    space.revoke_tree(root);
                }
            })
        })
        .collect();

    for h in handles {
        h.join().expect("worker joins");
    }

    // Final sweep: every non-root cap should be reachable from root
    // via parent chain. After revoke_tree(root), nothing but the
    // root can survive.
    let final_removed = space.revoke_tree(root);
    let remaining = space.enumerate();
    let live_count = remaining.len();

    // On pre-Phase-5 code, an orphan cap (parent=root, but root is
    // gone) survives the final revoke_tree and `remaining.len() > 1`.
    // On Phase-5 code, the orphan can't exist, so at most the root
    // remains (if we somehow missed revoking it earlier). After
    // our final revoke_tree, the root is gone too.
    assert!(
        live_count == 0,
        "ORPHAN REPRODUCED (D2): {live_count} cap(s) survived revoke_tree(root): {:?}",
        remaining
    );

    // The final revoke_tree removed at least 1 (the root, if it was
    // still there).
    assert!(
        final_removed >= 1 || live_count == 0,
        "final_removed={final_removed}, live_count={live_count}"
    );

    // Suppress unused-variable warnings.
    let _ = SlotId::new;
}

fn boot() -> (CapabilitySpace, CapabilityFactory) {
    let space = CapabilitySpace::new();
    let factory = CapabilityFactory::new(space.clone());
    (space, factory)
}

fn mint_counter(factory: &CapabilityFactory, name: &str) -> SlotId {
    let decl = CapabilityDecl {
        name: name.into(),
        in_type: "object".into(),
        out_type: "object".into(),
        streaming: false,
        ..Default::default()
    };
    let plugin = PluginId {
        name: "counter".into(),
        version: "0.1.0".into(),
    };
    factory.mint::<CounterResource>(
        CapKind::Sync,
        &decl,
        &plugin,
        CapabilityBudget::new(5000),
        odyssey::plugins::test_only::counter::handler(),
    )
}
