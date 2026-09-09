//! Lab — Same Program, Different Authority (Phase 2 P5).
//!
//! Two `RuleAgent` instances are constructed with **the same program
//! code** but **different capability environments**:
//!
//! ```text
//! Agent A  (READ only)
//!   └── slot:counter  ops = READ
//!
//! Agent B  (READ | WRITE)
//!   └── slot:counter  ops = READ | WRITE
//! ```
//!
//! Both agents try the same operations:
//!
//! - Agent A: read ✓, increment ✗, reset ✗
//! - Agent B: read ✓, increment ✓, reset ✗
//!
//! The agent itself doesn't decide whether increment is allowed —
//! `Capability::invoke_op` does, via the kernel-level guard.

use crate::capability::{CapabilityRights, OperationRights};
use crate::host::factory::CapabilityFactory;
use crate::plugins::agent::{handler as agent_handler, AgentResource};
use crate::plugins::counter::CounterResource;
use crate::capability::Slot;
use serde_json::json;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n== Lab :: Same Program, Different Authority ==\n");

    let cspace = crate::capability::CapabilitySpace::new();
    let factory = CapabilityFactory::new(cspace.clone());

    // Mint a counter with full authority.
    let pid = crate::host::manifest::PluginId {
        name: "counter".into(),
        version: "0.1.0".into(),
    };
    let counter_decl = crate::host::manifest::CapabilityDecl {
        name: "counter".into(),
        in_type: "object".into(),
        out_type: "object".into(),
        streaming: false,
    };
    let counter_slot = factory.mint::<CounterResource>(
        crate::capability::CapKind::Sync,
        &counter_decl,
        &pid,
        crate::capability::CapabilityBudget::new(5000),
        crate::plugins::counter::handler(),
    );

    // Agent A — counter restricted to READ only.
    let cap_a = cspace.restrict::<CounterResource>(
        counter_slot,
        CapabilityRights {
            operations: OperationRights::READ,
            timeout_ms: 5000,
        },
        "counter_for_agent_a".into(),
    )?;
    let agent_a_decl = crate::host::manifest::CapabilityDecl {
        name: "agent_a".into(),
        in_type: "object".into(),
        out_type: "object".into(),
        streaming: false,
    };
    let agent_pid = crate::host::manifest::PluginId {
        name: "agent".into(),
        version: "0.1.0".into(),
    };
    let agent_a_slot = factory.mint::<AgentResource>(
        crate::capability::CapKind::Sync,
        &agent_a_decl,
        &agent_pid,
        crate::capability::CapabilityBudget::new(5000),
        agent_handler(
            "agent_a".to_string(),
            vec![("counter".to_string(), cap_a)],
            cspace.clone(),
        ),
    );

    // Agent B — counter restricted to READ | WRITE.
    let cap_b = cspace.restrict::<CounterResource>(
        counter_slot,
        CapabilityRights {
            operations: OperationRights::READ | OperationRights::WRITE,
            timeout_ms: 5000,
        },
        "counter_for_agent_b".into(),
    )?;
    let agent_b_decl = crate::host::manifest::CapabilityDecl {
        name: "agent_b".into(),
        in_type: "object".into(),
        out_type: "object".into(),
        streaming: false,
    };
    let agent_b_slot = factory.mint::<AgentResource>(
        crate::capability::CapKind::Sync,
        &agent_b_decl,
        &agent_pid,
        crate::capability::CapabilityBudget::new(5000),
        agent_handler(
            "agent_b".to_string(),
            vec![("counter".to_string(), cap_b)],
            cspace.clone(),
        ),
    );

    let agent_a = Slot::<AgentResource>::new(cspace.clone(), agent_a_slot);
    let agent_b = Slot::<AgentResource>::new(cspace.clone(), agent_b_slot);

    println!("  agent_a slot={agent_a_slot}  (counter ops = READ)");
    println!("  agent_b slot={agent_b_slot}  (counter ops = READ | WRITE)");

    // Same inputs, both agents.
    let cases = [
        ("read", "read"),
        ("increment", "write"),
        ("reset", "admin"),
    ];

    println!("\n  Agent A (READ only):");
    for (action, _op) in cases.iter() {
        let r = agent_a.invoke(json!({ "target": "counter", "op": action }));
        let mark = if r.is_ok() { "✓" } else { "✗" };
        let detail = match r {
            Ok(v) => format!("ok: {v}"),
            Err(e) => format!("err: {e}"),
        };
        println!("    {mark} {action}: {detail}");
    }

    println!("\n  Agent B (READ | WRITE):");
    for (action, _op) in cases.iter() {
        let r = agent_b.invoke(json!({ "target": "counter", "op": action }));
        let mark = if r.is_ok() { "✓" } else { "✗" };
        let detail = match r {
            Ok(v) => format!("ok: {v}"),
            Err(e) => format!("err: {e}"),
        };
        println!("    {mark} {action}: {detail}");
    }

    println!("\n  same program; different caps → different reachable world.");
    println!("  done.");
    Ok(())
}