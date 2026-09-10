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

/// Test 3 — concurrent stress (smoke): many grant/revoke_tree
/// threads; final invariant is that `enumerate()` never reports a
/// non-root cap whose parent chain doesn't reach a root.
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
///
/// Kept as a smoke test using `std::thread` for the default build.
/// The loom-driven exhaustive permutation of the same race lives in
/// the `loom_concurrent_grant_and_revoke_tree` test below — it is
/// gated on the `loom-tests` feature so the default build does not
/// pay for loom's permutation engine.
#[cfg(not(feature = "loom-tests"))]
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

    assert!(
        live_count == 0,
        "ORPHAN REPRODUCED (D2): {live_count} cap(s) survived revoke_tree(root): {:?}",
        remaining
    );

    assert!(
        final_removed >= 1 || live_count == 0,
        "final_removed={final_removed}, live_count={live_count}"
    );
}

/// Test 3 (loom) — exhaustive permutation of the install_derived
/// / revoke_tree race. Loom's permutation engine replays the test
/// for every legal C11 ordering of the atomic operations, so any
/// interleaving that would commit an orphan is exercised here.
///
/// The production `CapabilitySpace` is built on `std::sync::RwLock`
/// / `std::sync::Arc`, which loom cannot permute. To exercise the
/// race under loom we drive a minimal model that captures the same
/// lock-order invariant (`parents → slots → names`):
///
///   - Two loom-friendly cells: `parents` and `slots`, both
///     `loom::sync::Mutex<HashSet<u64>>`.
///   - Thread A: install child under parent (acquire parents →
///     slots in canonical order, check `slots.contains(parent)`).
///   - Thread B: revoke parent (acquire parents → slots in
///     canonical order, drop the parent from `slots`).
///
/// Phase 5 invariant: if Thread B fully completes the revoke
/// before Thread A acquires `parents`, Thread A's precondition
/// check (`slots.contains(parent)`) fails and the install is
/// refused. If Thread A acquires `parents` first, Thread B blocks
/// on `parents` until Thread A commits; either the install lands
/// while the parent is alive (revoke then sweeps the child) or
/// the install lands after the revoke (impossible under canonical
/// ordering). In every permutation, exactly zero orphans survive.
#[cfg(feature = "loom-tests")]
#[test]
fn loom_concurrent_grant_and_revoke_tree_yields_no_orphans() {
    use loom::sync::Arc;
    use loom::thread;

    loom::model(|| {
        let model = Arc::new(Model::default_unboxed());

        let parent_id: u64 = 1;
        let child_id: u64 = 2;

        // Pre-populate the parent so the install can succeed
        // before the revoke lands.
        {
            let mut slots = model.slots.lock().unwrap();
            slots.insert(parent_id);
        }

        let model_a = Arc::clone(&model);
        let model_b = Arc::clone(&model);

        let handle_a = thread::spawn(move || {
            // install_derived: canonical lock order parents → slots.
            let _parents = model_a.parents_handle.lock().unwrap();
            let mut slots = model_a.slots.lock().unwrap();
            if slots.contains(&parent_id) {
                slots.insert(child_id);
                let mut parents_of = model_a.parents_of.lock().unwrap();
                parents_of.insert(child_id, parent_id);
            }
        });

        let handle_b = thread::spawn(move || {
            // revoke_tree: walk children, then remove root.
            let _parents = model_b.parents_handle.lock().unwrap();
            // First pass: collect descendants of parent_id.
            let descendants: Vec<u64> = {
                let parents_of = model_b.parents_of.lock().unwrap();
                parents_of
                    .iter()
                    .filter_map(|(child, parent)| {
                        if *parent == parent_id || is_descendant(*child, *parent, &parents_of) {
                            Some(*child)
                        } else {
                            None
                        }
                    })
                    .collect()
            };
            // Drop descendants, the root, and clear parent pointers.
            let mut slots = model_b.slots.lock().unwrap();
            let mut parents_of = model_b.parents_of.lock().unwrap();
            for d in &descendants {
                slots.remove(d);
                parents_of.remove(d);
            }
            slots.remove(&parent_id);
        });

        handle_a.join().unwrap();
        handle_b.join().unwrap();

        // Final invariant: every slot in `slots` either has no
        // parent recorded (a root) or its parent is also in
        // `slots`. No orphan survives.
        let slots = model.slots.lock().unwrap();
        let parents_of = model.parents_of.lock().unwrap();
        for child in slots.iter() {
            if let Some(&parent) = parents_of.get(child) {
                assert!(
                    slots.contains(&parent),
                    "ORPHAN REPRODUCED under loom: child={child} parent={parent} not in slots={:?}",
                    slots
                );
            }
        }
    });
}

#[cfg(feature = "loom-tests")]
fn is_descendant(
    needle: u64,
    candidate_parent: u64,
    parents_of: &std::collections::HashMap<u64, u64>,
) -> bool {
    if needle == candidate_parent {
        return true;
    }
    let mut current = needle;
    while let Some(&p) = parents_of.get(&current) {
        if p == candidate_parent {
            return true;
        }
        current = p;
    }
    false
}

/// Minimal loom-friendly model that mirrors the cspace's
/// `parents → slots → names` canonical lock acquisition order.
///
/// Each slot tracks its children so `revoke_tree` can walk the
/// subtree atomically and sweep every descendant in one critical
/// section — mirroring production `revoke_tree` which walks
/// `parents` under `parents.write()` and then calls
/// `revoke_single` for each.
///
/// The actual invariant under test: no orphan (a slot whose
/// parent is not in `slots`) survives after both threads finish.
#[cfg(feature = "loom-tests")]
struct Model {
    /// Serialises install + revoke (canonical "parents" lock).
    parents_handle: loom::sync::Mutex<()>,
    /// Slot map: id → present. We track children via the
    /// `parents_of` map; this is the "slots" half of the
    /// canonical lock order.
    slots: loom::sync::Mutex<std::collections::HashSet<u64>>,
    /// Parent pointer map: child → parent.
    parents_of: loom::sync::Mutex<std::collections::HashMap<u64, u64>>,
}

#[cfg(feature = "loom-tests")]
impl Model {
    fn default_unboxed() -> Self {
        Self {
            parents_handle: loom::sync::Mutex::new(()),
            slots: loom::sync::Mutex::new(std::collections::HashSet::new()),
            parents_of: loom::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }
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
        Arc::new(odyssey::kernel::SystemClock),
    )
}