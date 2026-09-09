//! Phase 1 property tests — the six properties the capability model
//! must satisfy.
//!
//! These tests are the **completion criterion** for Phase 1: when all
//! six pass, the seL4-style capability model is provably real in
//! `odyssey`.
//!
//! Each test builds a fresh `CapabilitySpace` + `CapabilityFactory`
//! (no cordis, no plugins, no HTTP bridge) so it can run in
//! isolation as part of `cargo test`.
//!
//! ## The six properties
//!
//! 1. **Authority** — a `Capability<R>` consulted with the right bit
//!    runs the handler; consulted without it is denied.
//! 2. **Delegation** — a plugin (`BrokerResource`) holding a
//!    capability can mint a derived slot via `cspace.restrict`.
//! 3. **Restrict / attenuation** — `cspace.restrict(child ⊇ parent)`
//!    returns `AttenuationViolation`.
//! 4. **Revocation** — `cspace.revoke(slot_id)` makes subsequent
//!    `Slot::invoke` calls fail; the slot reference itself stays
//!    valid (no panic).
//! 5. **Budget** — a `Slow` resource exceeds its budget returns a
//!    timeout error; a generous budget succeeds.
//! 6. **Composition** — slot-bound pipeline stages observe
//!    revocation mid-run via `PipelineError::SlotRevoked`.
//!
//! The Counter, Slow, and Echo resources are reused from the plugin
//! tree so the tests exercise the same code path as the runtime.

use odyssey::capability::{
    CapabilityBudget, CapabilityError, CapabilityRights, CapabilitySpace, CapKind,
    OperationRights, Slot, SlotId,
};
use odyssey::host::factory::CapabilityFactory;
use odyssey::host::manifest::{CapabilityDecl, PluginId};
use odyssey::host::pipeline::{Pipeline, SyncStage};
use odyssey::plugins::counter::CounterResource;
use odyssey::plugins::echo::EchoResource;
use odyssey::plugins::slow::SlowResource;
use serde_json::json;

/// Helper: mint a Counter cap with full authority.
fn mint_counter(space: &CapabilitySpace) -> SlotId {
    let factory = CapabilityFactory::new(space.clone());
    factory.mint::<CounterResource>(
        CapKind::Sync,
        &CapabilityDecl {
            name: "counter".into(),
            in_type: "object".into(),
            out_type: "object".into(),
            streaming: false,
        },
        &PluginId {
            name: "counter".into(),
            version: "0.1.0".into(),
        },
        CapabilityBudget::new(5000),
        odyssey::plugins::counter::handler(),
    )
}

fn mint_echo(space: &CapabilitySpace, name: &str) -> SlotId {
    let factory = CapabilityFactory::new(space.clone());
    factory.mint::<EchoResource>(
        CapKind::Sync,
        &CapabilityDecl {
            name: name.into(),
            in_type: "any".into(),
            out_type: "any".into(),
            streaming: false,
        },
        &PluginId {
            name: "echo".into(),
            version: "0.1.0".into(),
        },
        CapabilityBudget::new(5000),
        odyssey::plugins::echo::handler(),
    )
}

fn mint_slow(space: &CapabilitySpace) -> SlotId {
    let factory = CapabilityFactory::new(space.clone());
    factory.mint::<SlowResource>(
        CapKind::Sync,
        &CapabilityDecl {
            name: "slow".into(),
            in_type: "any".into(),
            out_type: "any".into(),
            streaming: false,
        },
        &PluginId {
            name: "slow".into(),
            version: "0.1.0".into(),
        },
        CapabilityBudget::new(5000),
        odyssey::plugins::slow::handler(),
    )
}

// ---------------------------------------------------------------------------
// 1. Authority
// ---------------------------------------------------------------------------

#[test]
fn authority_bits_gate_invocation() {
    let space = CapabilitySpace::new();
    let root_slot = mint_counter(&space);
    let cap = Slot::<CounterResource>::new(space.clone(), root_slot)
        .capability()
        .expect("root populated");

    // All four operations work on a root cap.
    assert!(cap.invoke_op(OperationRights::READ, json!({"op":"read"})).is_ok());
    assert!(cap.invoke_op(OperationRights::WRITE, json!({"op":"increment"})).is_ok());
    assert!(cap.invoke_op(OperationRights::ADMIN, json!({"op":"reset"})).is_ok());

    // Restrict the cap to READ only.
    let child_id = space
        .restrict::<CounterResource>(
            root_slot,
            CapabilityRights {
                operations: OperationRights::READ,
                timeout_ms: 5000,
            },
            "counter_readonly".into(),
        )
        .expect("restrict ok");
    let child_cap = Slot::<CounterResource>::new(space.clone(), child_id)
        .capability()
        .expect("child populated");

    assert!(child_cap.invoke_op(OperationRights::READ, json!({"op":"read"})).is_ok());
    assert!(child_cap
        .invoke_op(OperationRights::WRITE, json!({"op":"increment"}))
        .is_err());
    assert!(child_cap
        .invoke_op(OperationRights::ADMIN, json!({"op":"reset"}))
        .is_err());
}

// ---------------------------------------------------------------------------
// 2. Delegation
// ---------------------------------------------------------------------------

#[test]
fn delegation_through_broker_mints_child_slot() {
    use odyssey::plugins::broker::BrokerResource;

    let space = CapabilitySpace::new();
    let factory = CapabilityFactory::new(space.clone());

    // Mint counter with full authority.
    let counter_slot = mint_counter(&space);
    let counter_cap = Slot::<CounterResource>::new(space.clone(), counter_slot)
        .capability()
        .expect("counter cap");

    // Mint the broker resource, which closes over the counter cap.
    let broker_slot = factory.mint::<BrokerResource>(
        CapKind::Sync,
        &CapabilityDecl {
            name: "broker".into(),
            in_type: "object".into(),
            out_type: "object".into(),
            streaming: false,
        },
        &PluginId {
            name: "broker".into(),
            version: "0.1.0".into(),
        },
        CapabilityBudget::new(5000),
        odyssey::plugins::broker::handler(counter_cap.clone(), counter_slot, space.clone()),
    );

    let broker = Slot::<BrokerResource>::new(space.clone(), broker_slot);

    // 1) describe — broker reports what it holds.
    let described = broker
        .invoke(json!({"op": "describe"}))
        .expect("describe works");
    assert_eq!(described["held_name"], "counter");
    assert!(described["held_timeout_ms"].as_u64().unwrap() > 0);

    // 2) delegate — broker mints a child slot in the same CSpace.
    let delegated = broker
        .invoke(json!({
            "op": "delegate",
            "name": "counter_via_broker",
            "ops": ["READ"],
        }))
        .expect("delegate works");
    let new_slot_raw = delegated["slot"]
        .as_str()
        .and_then(|s| s.strip_prefix("slot:"))
        .and_then(|s| s.parse::<u64>().ok())
        .expect("slot id present");
    let new_slot = SlotId::new(new_slot_raw);

    // The child slot lives in the CSpace and works as a Counter slot.
    let child = Slot::<CounterResource>::new(space.clone(), new_slot);
    let child_cap = child.capability().expect("child slot populated");
    assert_eq!(child_cap.operations(), OperationRights::READ);
    assert!(child_cap
        .invoke_op(OperationRights::READ, json!({"op": "read"}))
        .is_ok());
    assert!(child_cap
        .invoke_op(OperationRights::WRITE, json!({"op": "increment"}))
        .is_err());
}

// ---------------------------------------------------------------------------
// 3. Restrict / attenuation
// ---------------------------------------------------------------------------

#[test]
fn restrict_rejects_authority_amplification() {
    let space = CapabilitySpace::new();
    let root_slot = mint_counter(&space);

    // Restrict to READ | WRITE.
    let child_id = space
        .restrict::<CounterResource>(
            root_slot,
            CapabilityRights {
                operations: OperationRights::READ | OperationRights::WRITE,
                timeout_ms: 5000,
            },
            "counter_partial".into(),
        )
        .expect("restrict down ok");

    // Attempt to *amplify* back to ALL — must fail.
    let amplify = space.restrict::<CounterResource>(
        child_id,
        CapabilityRights {
            operations: OperationRights::ALL,
            timeout_ms: 5000,
        },
        "counter_amplified".into(),
    );
    assert!(matches!(
        amplify,
        Err(CapabilityError::AttenuationViolation { .. })
    ));

    // Same with `grant`.
    let grant = space.grant::<CounterResource>(
        child_id,
        CapabilityRights {
            operations: OperationRights::EXECUTE | OperationRights::ADMIN,
            timeout_ms: 5000,
        },
        "counter_grant_attempt".into(),
    );
    assert!(matches!(grant, Err(CapabilityError::AttenuationViolation { .. })));

    // Same with `transfer`.
    let transfer = space.transfer::<CounterResource>(
        child_id,
        CapabilityRights {
            operations: OperationRights::ADMIN,
            timeout_ms: 5000,
        },
    );
    assert!(matches!(
        transfer,
        Err(CapabilityError::AttenuationViolation { .. })
    ));
}

// ---------------------------------------------------------------------------
// 4. Revocation
// ---------------------------------------------------------------------------

#[test]
fn revocation_invalidates_slot_without_panic() {
    let space = CapabilitySpace::new();
    let slot_id = mint_counter(&space);
    let slot = Slot::<CounterResource>::new(space.clone(), slot_id);

    // Sanity: pre-revoke, slot works.
    assert!(slot.invoke(json!({"op":"read"})).is_ok());

    // Revoke.
    assert!(space.revoke(slot_id));

    // The slot reference stays valid (no panic); lookup fails closed.
    let after = slot.capability();
    assert!(after.is_none());
    let err = slot.invoke(json!({"op":"read"})).unwrap_err();
    assert!(err.contains("slot") && err.contains("empty"), "got: {err}");

    // Revocation is idempotent.
    assert!(!space.revoke(slot_id));
}

// ---------------------------------------------------------------------------
// 5. Budget
// ---------------------------------------------------------------------------

#[test]
fn budget_constrains_wall_clock() {
    let space = CapabilitySpace::new();
    let slow_slot = mint_slow(&space);
    let slow = Slot::<SlowResource>::new(space.clone(), slow_slot);

    // Generous budget — SlowResource sleeps 200ms; budget 1000ms is fine.
    // First restrict to a generous budget, since the slow.toml timeout
    // gets re-applied by the factory (5000ms here). Direct invoke:
    // The handler returns its input; the budget fires after the call.
    let cap = slow.capability().expect("slow cap");
    let rights = cap.rights();
    assert!(rights.timeout_ms >= 200, "default budget must be ≥ handler sleep");

    // Tighter budget via restrict — 100ms while the handler sleeps 200ms.
    let tight_id = space
        .restrict::<SlowResource>(
            slow_slot,
            CapabilityRights {
                operations: rights.operations,
                timeout_ms: 100,
            },
            "slow_tight".into(),
        )
        .expect("restrict ok");
    let tight = Slot::<SlowResource>::new(space.clone(), tight_id);
    let err = tight.invoke(json!({})).unwrap_err();
    assert!(
        err.contains("timeout") || err.contains("budget"),
        "expected timeout denial, got: {err}"
    );

    // Generous budget (root) — succeeds.
    let ok = slow.invoke(json!({}));
    assert!(ok.is_ok(), "generous budget should succeed, got: {ok:?}");
}

// ---------------------------------------------------------------------------
// 6. Composition
// ---------------------------------------------------------------------------

#[test]
fn composition_observes_revocation_mid_run() {
    let space = CapabilitySpace::new();
    let echo_a = mint_echo(&space, "echo_a");
    let echo_b = mint_echo(&space, "echo_b");

    // Slot-bound pipeline so each stage re-resolves.
    let pipeline = Pipeline::new(vec![
        SyncStage::from_slot(echo_a, space.clone()).expect("stage a"),
        SyncStage::from_slot(echo_b, space.clone()).expect("stage b"),
    ]);

    // Before revoke — value flows through.
    let v = pipeline.run(json!({"k": "v"})).expect("first run ok");
    assert_eq!(v, json!({"k": "v"}));

    // Revoke A.
    assert!(space.revoke(echo_a));

    // After revoke — SlotRevoked.
    let err = pipeline.run(json!({"k": "v2"})).unwrap_err();
    assert!(
        matches!(err, odyssey::host::pipeline::PipelineError::SlotRevoked(_)),
        "expected SlotRevoked, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// Bonus: grant propagation semantics
// ---------------------------------------------------------------------------

#[test]
fn grant_preserves_source_capability() {
    let space = CapabilitySpace::new();
    let root_slot = mint_counter(&space);
    let root = Slot::<CounterResource>::new(space.clone(), root_slot);

    // Source works before grant.
    assert!(root.invoke(json!({"op":"read"})).is_ok());

    // Grant a child.
    let child_id = space
        .grant::<CounterResource>(
            root_slot,
            CapabilityRights {
                operations: OperationRights::READ,
                timeout_ms: 5000,
            },
            "counter_grant".into(),
        )
        .expect("grant ok");

    // Source still works (grant ≠ transfer).
    assert!(root.invoke(json!({"op":"read"})).is_ok());

    // Child works with restricted rights.
    let child = Slot::<CounterResource>::new(space.clone(), child_id);
    let child_cap = child.capability().expect("child populated");
    assert!(child_cap
        .invoke_op(OperationRights::READ, json!({"op": "read"}))
        .is_ok());
    assert!(child_cap
        .invoke_op(OperationRights::WRITE, json!({"op": "increment"}))
        .is_err());
}

#[test]
fn transfer_moves_and_clears_source() {
    let space = CapabilitySpace::new();
    let root_slot = mint_counter(&space);
    let root = Slot::<CounterResource>::new(space.clone(), root_slot);

    let moved_id = space
        .transfer::<CounterResource>(
            root_slot,
            CapabilityRights {
                operations: OperationRights::ALL,
                timeout_ms: 5000,
            },
        )
        .expect("transfer ok");

    // Source is empty.
    assert!(root.capability().is_none());

    // Destination has the cap.
    let moved = Slot::<CounterResource>::new(space.clone(), moved_id);
    assert!(moved.invoke(json!({"op":"read"})).is_ok());
}

// Silence the unused warning for `CapabilityDecl` (used above).