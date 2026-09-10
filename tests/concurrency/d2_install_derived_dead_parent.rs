//! D2 reproducer: `install_derived` can commit a dispatchable cap whose
//! parent slot has already been removed by a concurrent `revoke_tree`.
//!
//! This test runs the *exact* race window from the council Pass 2
//! walk-through. Three paths are exercised:
//!
//! - **Test 1 — install_after_revoke_tree_refuses_dead_parent**:
//!   pre-revoke the parent, then call `install_derived_for_test`
//!   directly (bypassing `grant`'s `lookup_typed` short-circuit).
//!   The install must fail because the parent no longer exists.
//!
//! - **Test 2 — install_after_revoke_single_refuses_dead_parent**:
//!   same idea but via `revoke_single` (single-slot revoke path,
//!   which also takes the canonical `parents → slots → names` lock
//!   order).
//!
//! - **Test 3 — concurrent_grant_during_revoke_tree**: many threads
//!   race; the post-condition is that no surviving non-root cap has
//!   a dead parent pointer. We approximate this by asserting that
//!   `enumerate()` never returns a non-root cap after the parent's
//!   revoke_tree completes (because every grant parented at the
//!   root must either be swept by the sweep or be refused at the
//!   install_derived precondition check).

use std::sync::Arc;

use odyssey::kernel::{
    Capability, CapabilityBudget, CapabilityRights, CapabilitySpace, CapKind, OperationRights,
    SlotId,
};
use odyssey::host::factory::CapabilityFactory;
use odyssey::host::manifest::{CapabilityDecl};
use odyssey::kernel::PluginId;
use odyssey::plugins::test_only::counter::CounterResource;

/// Test 1 — direct: revoke the parent first, then call
/// `install_derived` directly via the test-only accessor. The
/// production `grant` path goes through `lookup_typed` first,
/// which short-circuits with `SlotEmpty` before reaching
/// `install_derived` — so a test that only calls `grant` never
/// actually exercises the install_derived precondition check.
/// The test-only accessor (`install_derived_for_test`) exposes
/// the install path so this test can pin the precondition.
#[test]
fn install_after_revoke_tree_refuses_dead_parent() {
    let (space, factory) = boot();

    let root = mint_counter(&factory, "root");

    // Remove the parent via revoke_tree.
    let removed = space.revoke_tree(root);
    assert!(removed >= 1, "revoke_tree should remove at least the root");

    // Construct a fresh Capability<CounterResource> via the public
    // constructor. The cap construction itself doesn't require the
    // parent slot to be alive; only install_derived does.
    let derived = build_counter_cap("after_revoke");

    // Now try to install the derived cap directly. On pre-Phase-5
    // code, this would succeed and leave an orphan. On Phase-5
    // code, the precondition check fires.
    let install_result = space.install_derived_for_test::<CounterResource>(
        root,
        derived,
        "after_revoke".into(),
    );

    match install_result {
        Err(odyssey::kernel::CapabilityError::SlotEmpty(id)) => {
            assert_eq!(id, root, "SlotEmpty must surface the parent id");
        }
        Ok(new_slot) => {
            panic!(
                "INTERLEAVING 2 REPRODUCED: install_derived committed orphan at slot {} \
                 with parent {} (deleted); revoked=false",
                new_slot.raw(),
                root.raw()
            );
        }
        Err(e) => panic!("unexpected error variant: {e:?}"),
    }

    // Sanity: the root must be gone from the cspace.
    assert!(
        space.lookup_by_name("root").is_none(),
        "root must be revoked"
    );

    // And no orphan should be installed under the new name.
    assert!(
        space.lookup_by_name("after_revoke").is_none(),
        "orphan must NOT have been installed"
    );
}

/// Test 1b — same scenario via the `revoke_single` path. This
/// exercises the single-slot revoke path (which is what
/// `transfer` uses to clear the source after install_derived
/// moves the cap). Phase-5 invariant: `install_derived` refuses
/// on either revoke path because both take the canonical
/// `parents → slots → names` lock order.
#[test]
fn install_after_revoke_single_refuses_dead_parent() {
    let (space, factory) = boot();

    let root = mint_counter(&factory, "root");

    let removed = space.revoke(root);
    assert!(removed, "revoke (single) should succeed");

    let derived = build_counter_cap("after_single_revoke");

    let install_result = space.install_derived_for_test::<CounterResource>(
        root,
        derived,
        "after_single_revoke".into(),
    );

    assert!(
        matches!(install_result, Err(odyssey::kernel::CapabilityError::SlotEmpty(id)) if id == root),
        "install_derived must refuse on a single-revoked parent; got {install_result:?}"
    );
}

/// Test 2 — public grant path still returns `SlotEmpty` for a
/// pre-revoked parent (regression guard). The production code
/// goes through `derive_with → lookup_typed → install_derived`;
/// `lookup_typed` itself surfaces `SlotEmpty` when the parent
/// slot is gone. Either way the user-visible error is
/// `CapabilityError::SlotEmpty`.
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

    // Now try to grant a child of the dead root via the public API.
    let grant_result = space.grant::<CounterResource>(
        root,
        CapabilityRights {
            operations: OperationRights::READ,
            timeout_ms: 5000,
        },
        "after_revoke".into(),
    );

    match grant_result {
        Err(odyssey::kernel::CapabilityError::SlotEmpty(_)) => {
            // Phase-5 code: parent-live check fires.
        }
        Ok(new_slot) => {
            panic!(
                "INTERLEAVING 2 REPRODUCED via grant: orphan cap installed at slot {} \
                 with parent {} (deleted); revoked=false",
                new_slot.raw(),
                root.raw()
            );
        }
        Err(e) => panic!("unexpected error variant: {e:?}"),
    }

    assert!(
        space.lookup_by_name("root").is_none(),
        "root must be revoked"
    );
}

/// Test 3 — concurrent stress: many grant/revoke_tree threads; final
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

/// Build a fresh `Capability<CounterResource>` via the public
/// constructor. Used by the direct-install tests to feed
/// `install_derived_for_test` without going through `derive`
/// (which is `pub(crate)` — integration tests can't see it).
fn build_counter_cap(name: &str) -> Capability<CounterResource> {
    use odyssey::host::manifest::CapabilityDecl;
    use odyssey::kernel::ids::{CapabilityId, PluginId};
    use odyssey::kernel::meta::CapabilityMeta;
    use odyssey::kernel::quota::CapabilityBudget;
    use odyssey::kernel::{CapKind, CapabilityRights, OperationRights};

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
    let budget = CapabilityBudget::new(5000);
    let rights = CapabilityRights {
        operations: OperationRights::READ,
        timeout_ms: 5000,
    };
    let id = CapabilityId(0);
    let meta = CapabilityMeta {
        id: id.clone(),
        name: decl.name.clone(),
        namespace: plugin.name.clone(),
        contract_name: String::new(),
        plugin: plugin.clone(),
        in_type: decl.in_type.clone(),
        out_type: decl.out_type.clone(),
        streaming: decl.streaming,
        timeout_ms: budget.timeout_ms(),
        quota: budget.quota_spec(),
        authority: odyssey::kernel::AuthorityContract::default(),
        protocol: odyssey::kernel::Protocol::default(),
    };
    Capability::new(
        meta,
        odyssey::plugins::test_only::counter::handler(),
        budget,
        rights,
        CapKind::Sync,
    )
}