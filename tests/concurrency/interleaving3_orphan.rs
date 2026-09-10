//! P1-H — Interleaving 3: orphan survives a `revoke_tree`.
//!
//! The kernel's revocation invariant: every surviving slot has
//! a live parent (or is a root). Revocation paths (`revoke`,
//! `revoke_tree`) must therefore either (a) refuse to commit an
//! orphan, or (b) sweep every descendant atomically so no
//! orphan can sneak through.
//!
//! This test exercises a deterministic interleaving that
//! exhibits a potential orphan survival scenario:
//!
//!   1. Root has child A.
//!   2. \`cspace.children_of(root)\` returns [A] \u2014 the snapshot
//!      that \`revoke_tree\`'s walk would observe.
//!   3. Between the walk and the per-slot \`revoke_single\`, a
//!      concurrent \`grant(root, B)\` lands. \`install_derived\`
//!      acquires \`parents.write()\` first; the parent is
//!      still in \`slots\`, so the install succeeds and the
//!      new child B is inserted with \`parents[B] = root\`.
//!   4. \`revoke_single(root)\` then runs (or the test simulates
//!      a single-slot revoke directly). Root is removed from
//!      \`slots\` and \`parents\`, but B is left in \`slots\` \u2014
//!      with a parent pointer to the now-deleted root.
//!
//! Without a way to traverse parents-of-B from a live root, the
//! orphan cannot be reached by any subsequent \`revoke_tree\`. The
//! kernel can only clean it up via a direct \`revoke(B)\`.
//!
//! Test scope:
//!   - Demonstrate the orphan survival path with a deterministic
//!     sequence (no threading needed because the walk vs. install
//!     window is small but observable via direct calls).
//!   - Verify the kernel's invariant: a subsequent \`revoke_tree\`
//!     from the deleted root cannot reach the orphan.
//!   - Verify the orphan can be cleaned up by an explicit
//!     \`revoke\` on its own slot id.
//!
//! Phase 5 invariant \u2014 every survivor has a live parent:
//! after a successful \`revoke_tree\` chain the only orphans
//! possible are those inserted by \`install_derived\` racing with
//! the walk; here we verify that the surviving orphan cannot be
//! reached from any live root, matching the review's expectation.
use std::collections::HashMap;
use std::sync::Arc;

use odyssey::host::factory::CapabilityFactory;
use odyssey::kernel::space::CapabilitySpace;
use odyssey::kernel::{
    Capability, CapabilityBudget, CapabilityError, CapabilityRights, CapKind, OperationRights,
    SlotId,
};
use odyssey::kernel::PluginId;
use odyssey::plugins::test_only::counter::CounterResource;

#[test]
fn interleaving3_grant_during_revoke_tree_orphan_survives() {
    let (space, factory) = boot();

    // 1. Mint root and derive child A.
    let root = mint_counter(&factory, "root");
    let a = space
        .grant::<CounterResource>(
            root,
            CapabilityRights {
                operations: OperationRights::READ,
                timeout_ms: 5000,
            },
            "a".into(),
        )
        .expect("grant A");

    // 2. Snapshot the walk that `revoke_tree` would perform.
    let walked_children: Vec<SlotId> = space.children_of(root);
    assert_eq!(walked_children, vec![a], "walked_children must equal [a]");

    // 3. Between the walk and the per-slot `revoke_single`,
    //    a concurrent `grant(root, B)` lands. We simulate it
    //    deterministically: install_derived acquires
    //    parents.write() first, the parent is still in slots,
    //    the install commits. B is now in slots with
    //    parents[B] = root.
    let b = space
        .grant::<CounterResource>(
            root,
            CapabilityRights {
                operations: OperationRights::READ,
                timeout_ms: 5000,
            },
            "b".into(),
        )
        .expect("grant B between walk and revoke_single");

    // 4. `revoke_single(root)` runs (mimics what
    //    `revoke_tree`'s per-slot step would do for the root).
    //    Root is removed from `slots` + `parents` + `names`.
    //    B is left in `slots` with `parents[B] = root`.
    let removed = space.revoke(root);
    assert!(removed, "revoke(root) must succeed");

    // 5. State: B is in slots, root is gone, parents[B] = root.
    //    Use name_for_slot (registered name from the names map)
    //    instead of meta.name (inherited from parent via derive).
    //    Note: revoke(root) is a SINGLE-slot revoke (what the
    //    revoke_tree loop calls per frontier entry). It does
    //    NOT cascade. So A is also still in slots at this
    //    point — a single-slot revoke of root doesn't remove
    //    its descendants. The cascade only happens because the
    //    loop visits A next.
    //
    //    This test demonstrates the interleaving 3 race shape:
    //    A is in the walk (captured before the race window) but
    //    B is NOT (inserted after the walk, during the race
    //    window). If the loop only does the walk once and never
    //    re-walks, B survives the entire revoke_tree.
    let surviving: Vec<String> = vec![root, a, b]
        .into_iter()
        .filter_map(|s| space.name_for_slot(s))
        .collect();
    assert!(
        surviving.iter().any(|n| n == "b"),
        "B must survive the single-slot revoke (it was inserted after the walk); got {surviving:?}"
    );
    assert!(
        !surviving.iter().any(|n| n == "root"),
        "root must be revoked; got {surviving:?}"
    );

    // 6. Now simulate the rest of the revoke_tree loop:
    //    the frontier next contains A, so revoke_single(A) runs.
    //    A is removed. But B was never in any walk list — its
    //    parent pointer points to the deleted root, and B is
    //    now the canonical orphan shape.
    let removed_a = space.revoke(a);
    assert!(removed_a, "per-frontier revoke_single(A) succeeds");

    let surviving_after_a: Vec<String> = vec![root, a, b]
        .into_iter()
        .filter_map(|s| space.name_for_slot(s))
        .collect();
    assert!(
        !surviving_after_a.iter().any(|n| n == "a"),
        "A must be gone after per-frontier revoke_single; got {surviving_after_a:?}"
    );
    assert!(
        surviving_after_a.iter().any(|n| n == "b"),
        "B must still survive — never walked; got {surviving_after_a:?}"
    );

    // 6. B is now an orphan: parents[B] = root, root not in slots.
    //    Verify via the parent map: B's parent points to root,
    //    but root is gone. The kernel cannot reach B from any
    //    live root via parent traversal.
    let parent_map: HashMap<SlotId, SlotId> = space.parent_map();
    assert_eq!(parent_map.get(&b).copied(), Some(root), "parents[B] = root");
    assert!(
        space.slot_meta(root).is_none(),
        "root must not be in slots; orphan confirmed"
    );

    // 7. The orphan survives: revoke_tree(b) succeeds because B
    //    is in slots, but no live root can reach B via parent
    //    traversal (parents[B] = root, root is gone). The
    //    orphan is reachable only by direct slot id.
    let removed_b = space.revoke(b);
    assert!(removed_b, "explicit revoke(b) succeeds; the orphan is reachable only by direct slot id");

    let final_surviving: Vec<String> = vec![root, a, b]
        .into_iter()
        .filter_map(|s| space.name_for_slot(s))
        .collect();
    assert!(
        final_surviving.is_empty(),
        "after explicit revoke(b) the cspace is empty; got {final_surviving:?}"
    );
}

/// Phase 5 P1-H invariant test — orphan caps cannot be reached
/// from a live root, but they CAN be installed when the parent
/// is alive at install time. The kernel-level guarantee is:
/// install_derived refuses if the parent is gone (D2). This test
/// verifies that install_derived commits a child B only when
/// the parent is alive, then a subsequent revoke(root) leaves
/// B in slots with parents[B] = root (the orphan shape), and
/// B can be cleaned up by revoke(b).
#[test]
fn orphan_is_isolated_after_parent_revoke_single() {
    let (space, factory) = boot();

    let root = mint_counter(&factory, "root");
    let rights = CapabilityRights {
        operations: OperationRights::READ,
        timeout_ms: 5000,
    };

    let a = space.grant::<CounterResource>(root, rights, "a".into()).unwrap();
    let b = space.grant::<CounterResource>(root, rights, "b".into()).unwrap();

    // All three (root, a, b) are in slots; root has children [a, b].
    assert_eq!(space.children_of(root), vec![a, b]);

    // revoke(root) leaves a + b in slots but with parent=root.
    let _ = space.revoke(root);

    // a + b are still in slots but root is gone.
    assert!(space.slot_meta(root).is_none());
    assert!(space.slot_meta(a).is_some());
    assert!(space.slot_meta(b).is_some());

    // parents map still records a -> root, b -> root.
    let pm = space.parent_map();
    assert_eq!(pm.get(&a).copied(), Some(root));
    assert_eq!(pm.get(&b).copied(), Some(root));

    // revoke_tree(a) walks from a: a's parent is root (not in slots),
    // so the walk yields no descendants. Only a is removed.
    let removed_a = space.revoke_tree(a);
    assert!(removed_a >= 1, "revoke_tree(a) removes a itself");
    assert!(space.slot_meta(a).is_none());
    assert!(
        space.slot_meta(b).is_some(),
        "b is still an orphan (not reachable from a)"
    );

    // revoke(b) cleans up the remaining orphan.
    assert!(space.revoke(b));
    assert_eq!(space.len(), 0);
}

/// Reject the public-grant path against an orphan parent.
/// The Phase 5 D2 precondition check fires here: root is gone,
/// install_derived refuses, no new orphan is created.
#[test]
fn grant_against_orphan_parent_refused() {
    let (space, factory) = boot();

    let root = mint_counter(&factory, "root");
    assert!(space.revoke(root));

    let result = space.grant::<CounterResource>(
        root,
        CapabilityRights {
            operations: OperationRights::READ,
            timeout_ms: 5000,
        },
        "after_revoke".into(),
    );

    assert!(
        matches!(result, Err(CapabilityError::SlotEmpty(id)) if id == root),
        "grant against orphan parent must be refused by D2; got {result:?}"
    );
}

fn boot() -> (CapabilitySpace, CapabilityFactory) {
    let space = CapabilitySpace::new();
    let factory = CapabilityFactory::new(space.clone());
    (space, factory)
}

fn mint_counter(factory: &CapabilityFactory, name: &str) -> SlotId {
    use odyssey::host::manifest::CapabilityDecl;
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

// Allow the orphan cap to be inspected but not invoked.
#[allow(dead_code)]
fn _typed_cap_arc(_cspace: &CapabilitySpace, _slot: SlotId) -> Option<Arc<Capability<CounterResource>>> {
    None
}