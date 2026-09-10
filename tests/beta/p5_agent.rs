//! β.4 — Same program, different authority (P5).
//!
//! Two `RuleAgent` instances built from identical code but different
//! capability environments produce different observable behaviour.

use odyssey::kernel::{CapabilityRights, OperationRights, Slot};
use odyssey::plugins::agent::{agent_handler, AgentResource};
use odyssey::plugins::test_only::counter::CounterResource;
use serde_json::json;

#[test]
fn agent_gated_by_cap_authority() {
    let (space, factory) = crate::common::boot();
    let counter = crate::common::mint_counter(&factory);

    // Agent A's view: counter restricted to READ.
    let cap_a = space
        .restrict::<CounterResource>(
            counter,
            CapabilityRights {
                operations: OperationRights::READ,
                timeout_ms: crate::common::DEFAULT_TIMEOUT_MS,
            },
            "counter_a".into(),
        )
        .unwrap();
    // Agent B's view: counter restricted to READ | WRITE.
    let cap_b = space
        .restrict::<CounterResource>(
            counter,
            CapabilityRights {
                operations: OperationRights::READ | OperationRights::WRITE,
                timeout_ms: crate::common::DEFAULT_TIMEOUT_MS,
            },
            "counter_b".into(),
        )
        .unwrap();

    let pid = odyssey::kernel::PluginId {
        name: "agent".into(),
        version: "0.1.0".into(),
    };
    let decl_a = odyssey::host::manifest::CapabilityDecl {
        name: "agent_a".into(),
        in_type: "object".into(),
        out_type: "object".into(),
        streaming: false,
        ..Default::default()
    };
    let agent_a = factory.mint::<AgentResource>(
        odyssey::kernel::CapKind::Sync,
        &decl_a,
        &pid,
        odyssey::kernel::CapabilityBudget::new(crate::common::DEFAULT_TIMEOUT_MS),
        agent_handler("agent_a".to_string(), vec![("counter".to_string(), cap_a)], space.clone()),
    );
    let decl_b = odyssey::host::manifest::CapabilityDecl {
        name: "agent_b".into(),
        in_type: "object".into(),
        out_type: "object".into(),
        streaming: false,
        ..Default::default()
    };
    let agent_b = factory.mint::<AgentResource>(
        odyssey::kernel::CapKind::Sync,
        &decl_b,
        &pid,
        odyssey::kernel::CapabilityBudget::new(crate::common::DEFAULT_TIMEOUT_MS),
        agent_handler("agent_b".to_string(), vec![("counter".to_string(), cap_b)], space.clone()),
    );

    let a = Slot::<AgentResource>::new(space.clone(), agent_a);
    let b = Slot::<AgentResource>::new(space.clone(), agent_b);

    // read works for both.
    assert!(a.invoke(json!({"target": "counter", "op": "read"})).is_ok());
    assert!(b.invoke(json!({"target": "counter", "op": "read"})).is_ok());

    // increment: A denied, B succeeds.
    assert!(a.invoke(json!({"target": "counter", "op": "increment"})).is_err());
    assert!(b.invoke(json!({"target": "counter", "op": "increment"})).is_ok());

    // reset: both denied (neither holds ADMIN).
    assert!(a.invoke(json!({"target": "counter", "op": "reset"})).is_err());
    assert!(b.invoke(json!({"target": "counter", "op": "reset"})).is_err());
}