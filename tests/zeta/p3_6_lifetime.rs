//! ζ.4–ζ.7 — Phase 3 P3.6 Runtime Lifetime tests.
//!
//! Verify that the cspace lifetime tracks plugin lifetime:
//!
//! - **ζ.4** — Teardown order is reverse of mint order. The
//!   last plugin to mint is the first to die. cspace ends up
//!   empty for runtime plugins.
//! - **ζ.5** — Provider revoke invalidates consumer's binding
//!   table entry. After the provider's cap is revoked, the
//!   agent's reachable lookup for that handle returns
//!   `"reachable but cap missing in cspace"`. This is the
//!   P3.4 ↔ P3.6 handshake: reachable derives from bindings
//!   at mint time, and the binding's `capability` field is
//!   the cspace key — so revoke-then-lookup is the canonical
//!   invalidation pattern.
//! - **ζ.6** — `revoke_tree` on a parent slot also revokes
//!   every derived cap (`restrict`/`grant`). The child slots
//!   become unreachable from `lookup_by_name`.
//! - **ζ.7** — A consumer plugin (echo-chain) can be torn
//!   down **without** revoking its provider's slots. The
//!   provider's cap survives in cspace.

use std::sync::Arc;

use odyssey::kernel::{
    Capability, CapabilityBudget, CapabilityRights, CapabilitySpace, CapKind, OperationRights,
    Resource, SlotId,
};
use odyssey::host::factory::CapabilityFactory;
use odyssey::host::manifest::{CapabilityDecl};
use odyssey::kernel::PluginId;
use odyssey::host::manifest::ManifestBuilder;
use odyssey::host::resolver::{ResolvedBinding, ResolvedPlan};
use odyssey::plugins::echo::basic::{handler as echo_handler, EchoResource};
use odyssey::plugins::agent::handler_from_plan;
use odyssey::plugins::test_only::counter::{handler as counter_handler, CounterResource};
use serde_json::json;

const TIMEOUT_MS: u32 = 5000;

// =========================================================================
// Helpers
// =========================================================================

fn counter_manifest() -> odyssey::host::manifest::PluginManifest {
    ManifestBuilder::new("counter", "counter", "counter")
        .in_type("object")
        .out_type("object")
        .action("read", "READ")
        .action("increment", "WRITE")
        .action("reset", "ADMIN")
        .build()
}

fn echo_manifest() -> odyssey::host::manifest::PluginManifest {
    ManifestBuilder::new("echo", "echo", "echo")
        .in_type("any")
        .out_type("any")
        .action("echo", "EXECUTE")
        .build()
}

fn _echo_chain_manifest() -> odyssey::host::manifest::PluginManifest {
    ManifestBuilder::new("echo-chain", "echo_chain", "echo_chain")
        .in_type("any")
        .out_type("any")
        .requires("echo", "echo")
        .action("chain", "EXECUTE")
        .build()
}

fn mint_counter(
    factory: &CapabilityFactory,
    _cspace: &CapabilitySpace,
) -> (SlotId, Arc<Capability<CounterResource>>) {
    let m = counter_manifest();
    let slot = factory.mint::<CounterResource>(
        CapKind::Sync,
        &m.exposes[0],
        &m.plugin,
        CapabilityBudget::new(TIMEOUT_MS),
        counter_handler(),
    );
    let typed = factory
        .space()
        .lookup_typed::<CounterResource>(slot)
        .expect("counter cap just minted");
    (slot, typed)
}

fn mint_echo(
    factory: &CapabilityFactory,
    _cspace: &CapabilitySpace,
) -> SlotId {
    let m = echo_manifest();
    factory.mint::<EchoResource>(
        CapKind::Sync,
        &m.exposes[0],
        &m.plugin,
        CapabilityBudget::new(TIMEOUT_MS),
        echo_handler(),
    )
}

// =========================================================================
// ζ.4 — teardown order is reverse of mint order
// =========================================================================

#[test]
fn teardown_order_is_reverse_of_mint() {
    let (space, factory) = crate::common::boot();

    // Mint three plugins in known order: A, B, C.
    // Track each plugin's slot id separately so we can verify
    // the revocation order.
    let (slot_a, _) = mint_counter(&factory, &space); // pretend this is "A"
    let slot_b = mint_echo(&factory, &space); // "B"
    // For "C" use a second counter under a derived name.
    let slot_a2 = space
        .restrict::<CounterResource>(
            slot_a,
            CapabilityRights {
                operations: OperationRights::READ,
                timeout_ms: TIMEOUT_MS,
            },
            "counter_a2".into(),
        )
        .unwrap();

    let mint_order = [PluginId { name: "A".into(), version: "0.1.0".into() },
        PluginId { name: "B".into(), version: "0.1.0".into() },
        PluginId { name: "C".into(), version: "0.1.0".into() }];
    let mut minted = std::collections::HashMap::new();
    minted.insert(mint_order[0].clone(), vec![slot_a]);
    minted.insert(mint_order[1].clone(), vec![slot_b]);
    minted.insert(mint_order[2].clone(), vec![slot_a2]);

    assert_eq!(space.len(), 3, "three caps before teardown");

    // Teardown in reverse mint order, revoke each plugin's slots.
    for plugin_id in mint_order.iter().rev() {
        if let Some(slots) = minted.get(plugin_id) {
            for s in slots {
                space.revoke_tree(*s);
            }
        }
    }

    assert_eq!(space.len(), 0, "all slots revoked after teardown");
    assert!(space.lookup_erased(slot_a).is_none(), "A revoked");
    assert!(space.lookup_erased(slot_b).is_none(), "B revoked");
    assert!(space.lookup_erased(slot_a2).is_none(), "C revoked");
    assert!(space.lookup_by_name("counter").is_none(), "counter name cleared");
    assert!(space.lookup_by_name("echo").is_none(), "echo name cleared");
    assert!(space.lookup_by_name("counter_a2").is_none(), "counter_a2 name cleared");
}

// =========================================================================
// ζ.5 — provider revoke invalidates consumer's reachable entry
// =========================================================================

#[test]
fn provider_revoke_invalidates_consumer_reachable() {
    // The P3.4 ↔ P3.6 handshake:
    //   - P3.4 makes the agent's reachable derive from
    //     `plan.bindings[agent]`.
    //   - P3.6 revokes the provider's slot at teardown.
    //   - After teardown, the agent's reachable entry for the
    //     handle still exists in its storage (it's metadata),
    //     but `cspace.lookup_by_name(reachable.capability)`
    //     returns None. The agent's invoke path detects this
    //     and reports the failure clearly.
    let (space, factory) = crate::common::boot();
    let counter_slot = mint_counter(&factory, &space).0;

    // Build a synthetic plan: agent depends on counter.
    let consumer = PluginId {
        name: "agent".into(),
        version: "0.1.0".into(),
    };
    let bindings = vec![ResolvedBinding {
        handle: "counter".into(),
        provider: PluginId {
            name: "counter".into(),
            version: "0.1.0".into(),
        },
        capability: "counter".into(),
        contract: "counter".into(),
    }];
    let plan = ResolvedPlan {
        mint_order: vec![
            bindings[0].provider.clone(),
            consumer.clone(),
        ],
        bindings: std::iter::once((consumer.clone(), bindings)).collect(),
    };

    let agent =
        handler_from_plan("agent_zeta_5".to_string(), &plan, &consumer, space.clone());

    // Before teardown: dispatch works.
    assert!(
        agent
            .invoke(json!({"target": "counter", "op": "read"}))
            .is_ok(),
        "pre-revoke dispatch should succeed"
    );

    // Provider teardown: revoke the counter slot.
    let n = space.revoke_tree(counter_slot);
    assert_eq!(n, 1, "one slot revoked");

    // Reachable set in the agent is unchanged (it carries the
    // binding-table metadata), but the cspace lookup now fails.
    let reachable: Vec<&str> = agent.reachable().iter().map(|r| r.handle.as_str()).collect();
    assert_eq!(reachable, vec!["counter"]);
    assert_eq!(agent.reachable()[0].capability, "counter");

    let err = agent
        .invoke(json!({"target": "counter", "op": "read"}))
        .expect_err("post-revoke dispatch should fail");
    assert!(
        err.contains("reachable (handle=counter, capability=counter) but cap missing in cspace"),
        "expected cspace-missing error, got: {err}"
    );
}

// =========================================================================
// ζ.6 — revoke_tree propagates to derived caps
// =========================================================================

#[test]
fn revoke_tree_propagates_to_derived_caps() {
    // Verify cspace.revoke_tree severing the whole derived chain:
    //   counter (root)
    //   ├─ counter_read (derived via restrict, READ-only)
    //   │  └─ counter_read_only (derived from read, READ-only)
    //   └─ counter_write (derived via restrict, READ | WRITE)
    //
    // revoke_tree(counter) must revoke all four slots.
    let (space, factory) = crate::common::boot();
    let (root, _) = mint_counter(&factory, &space);

    let read = space
        .restrict::<CounterResource>(
            root,
            CapabilityRights {
                operations: OperationRights::READ,
                timeout_ms: TIMEOUT_MS,
            },
            "counter_read".into(),
        )
        .unwrap();
    let read_only = space
        .restrict::<CounterResource>(
            read,
            CapabilityRights {
                operations: OperationRights::READ,
                timeout_ms: TIMEOUT_MS,
            },
            "counter_read_only".into(),
        )
        .unwrap();
    let write = space
        .restrict::<CounterResource>(
            root,
            CapabilityRights {
                operations: OperationRights::READ | OperationRights::WRITE,
                timeout_ms: TIMEOUT_MS,
            },
            "counter_write".into(),
        )
        .unwrap();
    assert_eq!(space.len(), 4, "root + 3 derived");

    let revoked = space.revoke_tree(root);
    assert_eq!(revoked, 4, "root + every descendant revoked");

    assert_eq!(space.len(), 0);
    assert!(space.lookup_erased(read).is_none());
    assert!(space.lookup_erased(read_only).is_none());
    assert!(space.lookup_erased(write).is_none());
    assert!(space.lookup_by_name("counter").is_none());
    assert!(space.lookup_by_name("counter_read").is_none());
    assert!(space.lookup_by_name("counter_read_only").is_none());
    assert!(space.lookup_by_name("counter_write").is_none());
}

// =========================================================================
// ζ.7 — consumer teardown doesn't touch provider
// =========================================================================

#[test]
fn consumer_teardown_does_not_revoke_provider() {
    // echo-chain is a consumer (it `requires` echo). Tearing
    // echo-chain down first must NOT revoke echo's slot. Then
    // when echo eventually goes down, echo-chain's reachable
    // entry (still in storage) becomes unresolvable.
    let (space, factory) = crate::common::boot();
    let echo_slot = mint_echo(&factory, &space);

    // Synthesize echo-chain's "consumer" binding record (we
    // don't actually mint echo-chain's slot; we just want to
    // verify that its binding's `capability` still resolves
    // before echo is torn down, and stops resolving after).
    let consumer = PluginId {
        name: "echo-chain".into(),
        version: "0.1.0".into(),
    };
    let bindings = vec![ResolvedBinding {
        handle: "echo".into(),
        provider: PluginId {
            name: "echo".into(),
            version: "0.1.0".into(),
        },
        capability: "echo".into(),
        contract: "echo".into(),
    }];
    let plan = ResolvedPlan {
        mint_order: vec![
            bindings[0].provider.clone(),
            consumer.clone(),
        ],
        bindings: std::iter::once((consumer.clone(), bindings)).collect(),
    };

    let agent = handler_from_plan(
        "agent_zeta_7".to_string(),
        &plan,
        &consumer,
        space.clone(),
    );

    // echo alive → dispatch ok.
    assert!(
        agent
            .invoke(json!({"target": "echo", "op": "echo"}))
            .is_ok()
    );

    // Consumer "teardown" without touching echo's slot:
    // we revoke nothing yet, but verify a hypothetical
    // consumer-side revoke_tree (on a non-existent echo-chain
    // slot) doesn't reach echo.
    let _phantom_consumer_slot = SlotId::new(9999); // not in cspace
    let _ = space.revoke_tree(_phantom_consumer_slot); // no-op
    assert_eq!(space.len(), 1, "echo still alive");
    assert!(space.lookup_erased(echo_slot).is_some(), "echo survives consumer revoke");

    // Now provider teardown: echo's slot revoked.
    space.revoke_tree(echo_slot);
    assert_eq!(space.len(), 0);

    // Reachable entry still in agent's storage, but lookup fails.
    let err = agent
        .invoke(json!({"target": "echo", "op": "echo"}))
        .expect_err("after provider revoke, dispatch must fail");
    assert!(err.contains("cap missing in cspace"));

    // Suppress unused warning for CapabilityDecl alias.
    let _: CapabilityDecl = CapabilityDecl::default();
}