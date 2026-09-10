//! ζ.1–ζ.2 — Same `RuleAgent` binary, different binding tables,
//! different observable behaviour.
//!
//! The point of P3.4 is that the agent's *reachable set*
//! — the set of capability handles it can dispatch on —
//! is fully determined by what the resolver wired up in
//! `plan.bindings[agent_id]`. The agent's `invoke` never
//! reaches into the cspace for any handle that's not in
//! its reachable set, so two agents built from the SAME
//! `AgentResource::from_bindings` call but given two
//! DIFFERENT `ResolvedPlan`s behave observably differently.
//!
//! Setup helpers (in `tests/common/mod.rs`) mint real caps
//! via the factory and cspace. Each test builds its own
//! `ResolvedPlan` by hand using
//! `kernel::resolver::{ResolvedBinding, ResolvedPlan}` so we
//! can vary the binding table without spinning up the full
//! boot.

use std::sync::Arc;

use odyssey::capability::{CapabilitySpace, Resource, Slot};
use odyssey::kernel::manifest::{CapabilityDecl, PluginId};
use odyssey::kernel::manifest_builder::ManifestBuilder;
use odyssey::kernel::resolver::{resolve, ResolvedBinding, ResolvedPlan};
use odyssey::plugins::test_only::agent::{
    handler_from_plan, AgentResource, Reachable,
};
use odyssey::plugins::test_only::counter::CounterResource;
use serde_json::json;

const TIMEOUT_MS: u32 = 5000;

/// Build a manifest for an agent that declares `requires`.
/// Plugin id is fixed (`name="agent"`, `version="0.1.0"`)
/// because each test only mints one consumer.
fn agent_manifest_with_requires(
    requires: Vec<(&'static str, &'static str)>, // (handle, contract)
) -> odyssey::kernel::manifest::PluginManifest {
    let mut b = ManifestBuilder::new("agent", "agent", "agent")
        .in_type("object")
        .out_type("object");
    for (handle, contract) in requires {
        b = b.requires(handle, contract);
    }
    b.build()
}

/// Build a counter manifest. Same shape as the real one in
/// `src/plugins/test_only/counter/counter.toml`; tests build it
/// inline so the resolver walks our hand-rolled manifest rather
/// than reading the disk file (which would couple the test to
/// parser details). The three `.action(...)` calls publish the
/// `read → READ, increment → WRITE, reset → ADMIN` vocabulary
/// that the RuleAgent consults.
fn counter_manifest() -> odyssey::kernel::manifest::PluginManifest {
    ManifestBuilder::new("counter", "counter", "counter")
        .in_type("object")
        .out_type("object")
        .action("read", "READ")
        .action("increment", "WRITE")
        .action("reset", "ADMIN")
        .build()
}

/// Mint a counter cap and register it in cspace under the name
/// "counter". Returns the slot id.
fn mint_counter_for(
    factory: &odyssey::kernel::factory::CapabilityFactory,
    cspace: &CapabilitySpace,
) -> odyssey::capability::SlotId {
    use odyssey::capability::{CapabilityBudget, CapKind};
    let m = counter_manifest();
    let slot = factory.mint::<CounterResource>(
        CapKind::Sync,
        &m.exposes[0],
        &m.plugin,
        CapabilityBudget::new(TIMEOUT_MS),
        odyssey::plugins::test_only::counter::handler(),
    );
    // Force the cap's name into cspace's names map so
    // `lookup_by_name("counter")` works. (Factory mint already
    // does this when cspace is the install target.)
    let _ = cspace; // cspace is implicit via the factory
    slot
}

/// Mint a real `Capability<CounterResource>` and return both
/// the Arc and the slot id.
fn mint_counter_arc(
    factory: &odyssey::kernel::factory::CapabilityFactory,
) -> (
    odyssey::capability::SlotId,
    Arc<odyssey::capability::Capability<CounterResource>>,
) {
    use odyssey::capability::{CapabilityBudget, CapKind};
    let m = counter_manifest();
    let slot = factory.mint::<CounterResource>(
        CapKind::Sync,
        &m.exposes[0],
        &m.plugin,
        CapabilityBudget::new(TIMEOUT_MS),
        odyssey::plugins::test_only::counter::handler(),
    );
    let typed = factory
        .space()
        .lookup_typed::<CounterResource>(slot)
        .expect("counter cap just minted");
    (slot, typed)
}

/// Hand-build a `ResolvedPlan` from a list of bindings for a
/// single consumer. The mint_order has the providers first
/// (the counter plugin) then the agent; the bindings map
/// carries the agent's dependencies.
///
/// This bypasses the real resolver. Tests that need the real
/// resolver (`resolve(&manifests)`) are below.
fn synthetic_plan(
    consumer: &PluginId,
    bindings: Vec<ResolvedBinding>,
) -> ResolvedPlan {
    use std::collections::BTreeMap;
    let mut providers: Vec<PluginId> = bindings
        .iter()
        .map(|b| b.provider.clone())
        .collect();
    providers.sort();
    providers.dedup();
    let mut mint_order = providers;
    mint_order.push(consumer.clone());
    let mut map = BTreeMap::new();
    map.insert(consumer.clone(), bindings);
    ResolvedPlan {
        mint_order,
        bindings: map,
    }
}

// =========================================================================
// ζ.1.a — same binary, requires=[counter]
// =========================================================================

#[test]
fn same_binary_requires_counter_only_addresses_counter() {
    let (space, factory) = crate::common::boot();
    let _counter_slot = mint_counter_for(&factory, &space);

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
    let plan = synthetic_plan(&consumer, bindings);

    let agent =
        handler_from_plan("agent_zeta_1a".to_string(), &plan, &consumer, space.clone());

    // Reachable set came from bindings.
    let reachable = agent.reachable();
    assert_eq!(reachable.len(), 1);
    assert_eq!(reachable[0].handle, "counter");
    assert_eq!(reachable[0].capability, "counter");

    // counter is reachable → dispatch succeeds.
    let result = agent
        .invoke(json!({"target": "counter", "op": "read"}))
        .expect("counter read should succeed");
    assert_eq!(result["agent"], "agent_zeta_1a");
    assert_eq!(result["target"], "counter");

    // "echo" is NOT in reachable → rejected.
    let err = agent
        .invoke(json!({"target": "echo", "op": "execute"}))
        .expect_err("echo is not in reachable set");
    assert!(
        err.contains("not in the binding table"),
        "expected 'not in the binding table' error, got: {err}"
    );
    assert!(
        err.contains("\"counter\""),
        "error should mention what's reachable, got: {err}"
    );
}

// =========================================================================
// ζ.1.b — same binary, requires=[]
// =========================================================================

#[test]
fn same_binary_requires_empty_addresses_nothing() {
    let (space, factory) = crate::common::boot();
    let _counter_slot = mint_counter_for(&factory, &space);

    let consumer = PluginId {
        name: "agent".into(),
        version: "0.1.0".into(),
    };
    let plan = synthetic_plan(&consumer, vec![]);

    let agent =
        handler_from_plan("agent_zeta_1b".to_string(), &plan, &consumer, space.clone());

    // Reachable is empty.
    assert!(agent.reachable().is_empty());

    // Even a cap that exists in cspace is unreachable: agent
    // doesn't see it because its binding table is empty.
    let err = agent
        .invoke(json!({"target": "counter", "op": "read"}))
        .expect_err("empty reachable means every target rejected");
    assert!(err.contains("not in the binding table"));

    let err = agent
        .invoke(json!({"target": "anything", "op": "do"}))
        .expect_err("anything rejected");
    assert!(err.contains("not in the binding table"));
}

// =========================================================================
// ζ.1.c — same binary, requires=[counter, echo]
// =========================================================================

#[test]
fn same_binary_requires_two_handles_addresses_both() {
    // Mint both counter and echo.
    let (space, factory) = crate::common::boot();
    use odyssey::capability::{CapabilityBudget, CapKind};
    let _counter_slot = mint_counter_for(&factory, &space);
    let m_echo = odyssey::plugins::echo::basic::manifest();
    let _echo_slot = factory.mint::<odyssey::plugins::echo::basic::EchoResource>(
        CapKind::Sync,
        &m_echo.exposes[0],
        &m_echo.plugin,
        CapabilityBudget::new(TIMEOUT_MS),
        odyssey::plugins::echo::basic::handler(),
    );

    let consumer = PluginId {
        name: "agent".into(),
        version: "0.1.0".into(),
    };
    let bindings = vec![
        ResolvedBinding {
            handle: "counter".into(),
            provider: PluginId {
                name: "counter".into(),
                version: "0.1.0".into(),
            },
            capability: "counter".into(),
            contract: "counter".into(),
        },
        ResolvedBinding {
            handle: "echo".into(),
            provider: PluginId {
                name: "echo".into(),
                version: "0.1.0".into(),
            },
            capability: "echo".into(),
            contract: "echo".into(),
        },
    ];
    let plan = synthetic_plan(&consumer, bindings);

    let agent =
        handler_from_plan("agent_zeta_1c".to_string(), &plan, &consumer, space.clone());

    let names: Vec<&str> = agent.reachable().iter().map(|r| r.handle.as_str()).collect();
    assert_eq!(names, vec!["counter", "echo"]); // sorted

    // Both reachable.
    assert!(
        agent
            .invoke(json!({"target": "counter", "op": "read"}))
            .is_ok()
    );
    assert!(
        agent
            .invoke(json!({"target": "echo", "op": "echo"}))
            .is_ok()
    );

    // Something not in bindings → rejected.
    let err = agent
        .invoke(json!({"target": "sandbox", "op": "exec"}))
        .expect_err("sandbox not in bindings");
    assert!(err.contains("not in the binding table"));
}

// =========================================================================
// ζ.2 — same `target` handle, different `capability` field
// =========================================================================

#[test]
fn same_handle_different_capability_reaches_different_caps() {
    // This is the "P3.4 + authority" combo: two agents with the
    // SAME reachable handle ("counter"), but the binding's
    // `capability` field points to two DIFFERENT caps in
    // cspace. We use `restrict` to mint two derived caps from
    // the same counter source, named "counter_read" (READ-only)
    // and "counter_write" (READ | WRITE).
    use odyssey::capability::{CapabilityRights, OperationRights};
    let (space, factory) = crate::common::boot();
    let (_counter_slot, _counter_arc) = mint_counter_arc(&factory);

    let read_cap = space
        .restrict::<CounterResource>(
            _counter_slot,
            CapabilityRights {
                operations: OperationRights::READ,
                timeout_ms: TIMEOUT_MS,
            },
            "counter_read".into(),
        )
        .unwrap();
    let write_cap = space
        .restrict::<CounterResource>(
            _counter_slot,
            CapabilityRights {
                operations: OperationRights::READ | OperationRights::WRITE,
                timeout_ms: TIMEOUT_MS,
            },
            "counter_write".into(),
        )
        .unwrap();
    assert_ne!(read_cap, write_cap);

    let consumer = PluginId {
        name: "agent".into(),
        version: "0.1.0".into(),
    };

    // Agent A: binding handle="counter" → capability="counter_read".
    let plan_a = synthetic_plan(
        &consumer,
        vec![ResolvedBinding {
            handle: "counter".into(),
            provider: PluginId {
                name: "counter".into(),
                version: "0.1.0".into(),
            },
            capability: "counter_read".into(),
            contract: "counter".into(),
        }],
    );
    let agent_a =
        handler_from_plan("agent_zeta_2a".to_string(), &plan_a, &consumer, space.clone());

    // Agent B: binding handle="counter" → capability="counter_write".
    let plan_b = synthetic_plan(
        &consumer,
        vec![ResolvedBinding {
            handle: "counter".into(),
            provider: PluginId {
                name: "counter".into(),
                version: "0.1.0".into(),
            },
            capability: "counter_write".into(),
            contract: "counter".into(),
        }],
    );
    let agent_b =
        handler_from_plan("agent_zeta_2b".to_string(), &plan_b, &consumer, space.clone());

    // Reachable handles are identical (both "counter")...
    let handles_a: Vec<&str> = agent_a.reachable().iter().map(|r| r.handle.as_str()).collect();
    let handles_b: Vec<&str> = agent_b.reachable().iter().map(|r| r.handle.as_str()).collect();
    assert_eq!(handles_a, handles_b);
    assert_eq!(handles_a, vec!["counter"]);

    // ...but the underlying `capability` differs — pointing the
    // cspace lookup at different caps.
    assert_eq!(agent_a.reachable()[0].capability, "counter_read");
    assert_eq!(agent_b.reachable()[0].capability, "counter_write");

    // ...but the actual caps reached differ.
    let r_a = agent_a
        .invoke(json!({"target": "counter", "op": "increment"}))
        .expect_err("agent A has READ-only counter; increment fails");
    assert!(r_a.contains("READ") && r_a.contains("WRITE"),
        "expected WRITE-vs-held-READ message, got: {r_a}");

    let r_b = agent_b
        .invoke(json!({"target": "counter", "op": "increment"}))
        .expect("agent B has READ+WRITE counter; increment succeeds");
    assert_eq!(r_b["target"], "counter");
}

// =========================================================================
// ζ.3 — real resolver, real boot-shaped manifest graph
// =========================================================================

#[test]
fn real_resolver_drives_reachable_set() {
    // Two agents, identical binary, declared via real manifests.
    // The resolver walks the manifests and produces the binding
    // table. We feed that into AgentResource::from_bindings and
    // assert the agent sees exactly what the resolver wired up.
    let (space, factory) = crate::common::boot();
    let _counter_slot = mint_counter_for(&factory, &space);

    let counter_pid = PluginId {
        name: "counter".into(),
        version: "0.1.0".into(),
    };
    let consumer = PluginId {
        name: "agent".into(),
        version: "0.1.0".into(),
    };

    // Manifest graph: counter provider + agent consumer.
    let manifests = vec![
        counter_manifest(),
        agent_manifest_with_requires(vec![("counter", "counter")]),
    ];
    let plan = resolve(&manifests).expect("resolver accepts counter + agent");

    let agent =
        handler_from_plan("agent_zeta_3".to_string(), &plan, &consumer, space.clone());

    let reachable: Vec<&Reachable> = agent.reachable().iter().collect();
    assert_eq!(reachable.len(), 1);
    assert_eq!(reachable[0].handle, "counter");
    assert_eq!(reachable[0].capability, "counter");
    assert_eq!(
        AgentResource::binding_for(&plan, &consumer, "counter")
            .map(|b| b.provider.clone()),
        Some(counter_pid),
        "binding_for exposes the provider from the resolved plan"
    );

    // Smoke: dispatch on the reachable handle.
    let r = agent
        .invoke(json!({"target": "counter", "op": "read"}))
        .expect("counter reachable + minted → dispatch succeeds");
    assert_eq!(r["agent"], "agent_zeta_3");

    // Out-of-reach dispatch is rejected with a clear message.
    let err = agent
        .invoke(json!({"target": "echo", "op": "execute"}))
        .expect_err("echo is not declared in requires");
    assert!(err.contains("not in the binding table"));
    // Suppress unused warnings.
    let _: Slot<AgentResource>;
    let _: CapabilityDecl;
}