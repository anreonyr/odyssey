//! D3 reproducer: `share_quota_with` allocates a fresh wall-clock
//! counter, so derived caps do NOT accumulate wall-clock usage
//! against the parent's bucket.

use std::sync::Arc;

use odyssey::kernel::{
    Capability, CapabilityBudget, CapabilityRights, CapabilitySpace, CapKind, OperationRights,
};
use odyssey::host::factory::CapabilityFactory;
use odyssey::host::manifest::{CapabilityDecl};
use odyssey::kernel::PluginId;
use odyssey::plugins::test_only::counter::CounterResource;

/// On pre-Phase-5 code, the child's `wall_clock_total_ms` is fresh
/// and only reflects the child's own calls. After Phase 5, the child
/// shares the parent's `Arc<AtomicU64>`, so its counter reflects the
/// total subtree usage.
#[test]
fn child_wall_clock_inherits_parent_counter() {
    let (space, factory) = boot();

    // Root counter — full authority.
    let root = factory.mint::<CounterResource>(
        CapKind::Sync,
        &counter_decl("root"),
        &counter_pid(),
        CapabilityBudget::new(5000),
        odyssey::plugins::test_only::counter::handler(),
    );

    let typed_root: Arc<Capability<CounterResource>> = space
        .lookup_typed::<CounterResource>(root)
        .expect("root typed cap");

    // Burn some wall-clock on root before deriving.
    for _ in 0..10 {
        let _ = typed_root.invoke_op(
            OperationRights::READ,
            serde_json::json!({"op": "read"}),
        );
    }

    let root_after_10 = typed_root.budget().wall_clock_total_ms();
    assert!(
        root_after_10 > 0,
        "root should have accumulated wall-clock time, got {root_after_10}"
    );

    // Derive a read-only child.
    let child = space
        .grant::<CounterResource>(
            root,
            CapabilityRights {
                operations: OperationRights::READ,
                timeout_ms: 5000,
            },
            "child".into(),
        )
        .expect("grant succeeds");

    let typed_child: Arc<Capability<CounterResource>> = space
        .lookup_typed::<CounterResource>(child)
        .expect("child typed cap");

    // On pre-Phase-5 code, child starts at 0.
    // On Phase-5 code, child inherits root's counter.
    let child_before = typed_child.budget().wall_clock_total_ms();

    // Burn 10 more on child.
    for _ in 0..10 {
        let _ = typed_child.invoke_op(
            OperationRights::READ,
            serde_json::json!({"op": "read"}),
        );
    }

    let child_after = typed_child.budget().wall_clock_total_ms();
    let root_after = typed_root.budget().wall_clock_total_ms();

    // The child counter MUST reflect the subtree total: at least
    // (root's pre-derive counter) + (child's own calls). On
    // pre-Phase-5 code, child_before == 0 and child_after is small,
    // so the assertion below fails.
    assert!(
        child_before >= root_after_10,
        "PRE-PHASE-5 BUG (D3): child wall-clock counter did not inherit parent's; \
         child_before={child_before}, root_after_10={root_after_10}"
    );

    // The parent's counter must NOT have decreased.
    assert!(
        root_after >= root_after_10,
        "parent counter regressed: {root_after} < {root_after_10}"
    );

    // And the child MUST see at least its own 10 calls added on top
    // of whatever the parent had. (The exact number depends on
    // whether they share the same atomic; on Phase-5 code they do.)
    assert!(
        child_after > child_before,
        "child counter did not advance after 10 calls: child_before={child_before}, child_after={child_after}"
    );
}

fn boot() -> (CapabilitySpace, CapabilityFactory) {
    let space = CapabilitySpace::new();
    let factory = CapabilityFactory::new(space.clone());
    (space, factory)
}

fn counter_decl(name: &str) -> CapabilityDecl {
    CapabilityDecl {
        name: name.into(),
        in_type: "object".into(),
        out_type: "object".into(),
        streaming: false,
        ..Default::default()
    }
}

fn counter_pid() -> PluginId {
    PluginId {
        name: "counter".into(),
        version: "0.1.0".into(),
    }
}
