//! ζ.8–ζ.11 — Phase 3 P3.2 Protocol as Metadata tests.
//!
//! Verify that the split between `AuthorityContract` (action →
//! op map; load-bearing) and `Protocol` (wire metadata; pure
//! observation) is honoured: protocol changes don't affect
//! dispatch, authority changes do.
//!
//! - **ζ.8** — same binary, with/without protocol → same
//!   dispatch results. Protocol is metadata, not a contract.
//! - **ζ.9** — protocol is queryable from `cap.meta().protocol`.
//!   Wire-format metadata survives mint.
//! - **ζ.10** — `AuthorityContract::operation_for` is the only
//!   path the agent uses for action → bit translation.
//! - **ζ.11** — `ManifestBuilder::protocol(...)` roundtrips
//!   through the same fields as a hand-built struct.

use std::sync::Arc;

use odyssey::capability::{Capability, CapabilityBudget, CapabilitySpace, CapKind, Resource};
use odyssey::kernel::factory::CapabilityFactory;
use odyssey::kernel::manifest_builder::ManifestBuilder as MB;
use odyssey::plugins::test_only::counter::{handler as counter_handler, CounterResource};
use odyssey::capability::{AuthorityContract, Protocol};
use serde_json::{json, Value};

const TIMEOUT_MS: u32 = 5000;

// =========================================================================
// ζ.8 — same dispatch with/without protocol metadata
// =========================================================================

#[test]
fn dispatch_unchanged_with_or_without_protocol_metadata() {
    // Two caps with **identical authority** (same action
    // vocabulary) but **different protocol metadata** (different
    // description, schemas, transport, version). The runtime
    // dispatch path is the same — the cap's handler sees raw
    // `Value`, no schema validation, no transport hint. So both
    // caps must produce identical `Result<Value, String>` for
    // every input.
    let (space, factory) = crate::common::boot();
    let _cap_a = mint_counter_with_protocol(
        &factory,
        "counter_a",
        Protocol::empty()
            .with_description("Counter A")
            .with_media_type("application/json")
            .with_version("1.0")
            .with_transport("in-process"),
    );
    let _cap_b = mint_counter_with_protocol(
        &factory,
        "counter_b",
        Protocol::empty()
            .with_description("Counter B (different)")
            .with_media_type("application/x-msgpack") // different
            .with_version("2.5")                       // different
            .with_transport("http"),                    // different
    );

    // Both caps carry the same authority (read→READ, increment
//WRITE, reset→ADMIN) via the factory's helper.
    // Inspect metadata first to make the test readable.
    let meta_a = space.slot_meta(_cap_a).unwrap();
    let meta_b = space.slot_meta(_cap_b).unwrap();
    assert_ne!(meta_a.protocol.media_type, meta_b.protocol.media_type);
    assert_ne!(meta_a.protocol.version, meta_b.protocol.version);
    assert_ne!(meta_a.protocol.transport, meta_b.protocol.transport);
    // But authority is identical.
    assert_eq!(meta_a.authority.actions.len(), meta_b.authority.actions.len());

    // Now invoke each cap with the same input via a Slot.
    // Since protocol is metadata-only, both caps produce the
    // same Result for identical input.
    let slot_a = odyssey::capability::Slot::<CounterResource>::new(space.clone(), _cap_a);
    let slot_b = odyssey::capability::Slot::<CounterResource>::new(space.clone(), _cap_b);
    let read_a = slot_a.invoke(json!({"op": "read"}));
    let read_b = slot_b.invoke(json!({"op": "read"}));
    assert_eq!(read_a, read_b, "protocol metadata must not affect read result");

    let inc_a = slot_a.invoke(json!({"op": "increment"}));
    let inc_b = slot_b.invoke(json!({"op": "increment"}));
    assert_eq!(inc_a, inc_b, "protocol metadata must not affect increment result");
}

// =========================================================================
// ζ.9 — protocol is queryable from cap.meta().protocol
// =========================================================================

#[test]
fn protocol_queryable_from_cap_meta() {
    let (space, factory) = crate::common::boot();
    let protocol = Protocol::empty()
        .with_description("Shared counter for tests")
        .with_input(json!({"type": "object"}))  // placeholder
        .with_output(json!({"type": "integer"}))
        .with_media_type("application/json")
        .with_version("1.0")
        .with_transport("in-process");
    let slot = mint_counter_with_protocol(&factory, "counter_q", protocol.clone());

    let meta = space.slot_meta(slot).expect("slot has meta");
    assert_eq!(meta.protocol.description, protocol.description);
    assert_eq!(meta.protocol.media_type, "application/json");
    assert_eq!(meta.protocol.version, "1.0");
    assert_eq!(meta.protocol.transport, "in-process");
    assert_eq!(meta.protocol.input_schema, json!({"type": "object"}));
    assert_eq!(meta.protocol.output_schema, json!({"type": "integer"}));
}

// =========================================================================
// ζ.10 — AuthorityContract is the only authority source
// =========================================================================

#[test]
fn authority_operation_for_is_action_vocabulary() {
    // Phase 3 P3.2: the agent reads
    // `meta.authority.operation_for(action)`. Verify the public
    // vocabulary exactly matches what the agent uses.
    let a = AuthorityContract::empty()
        .with_action("read", "READ")
        .with_action("increment", "WRITE")
        .with_action("reset", "ADMIN");

    assert_eq!(a.operation_for("read"), Some("READ"));
    assert_eq!(a.operation_for("increment"), Some("WRITE"));
    assert_eq!(a.operation_for("reset"), Some("ADMIN"));
    assert_eq!(a.operation_for("not_published"), None);
    assert_eq!(a.operation_for(""), None);
    assert_eq!(a.actions.len(), 3);
}

#[test]
fn authority_vocabulary_drives_dispatch_authority() {
    // End-to-end: the agent consults `meta.authority.operation_for`,
    // gets the bit string, parses it, and checks against the cap's
    // held operations. We verify the integration works by
    // (a) building a real cap with an authority contract,
    // (b) constructing an AgentResource whose reachable entry
    //     points at this cap,
    // (c) dispatching a `read` action and verifying it succeeds
    //     (the agent reads "READ" from authority.operation_for,
    //     finds the cap holds READ, and invokes).
    use odyssey::kernel::manifest::PluginId;
    use odyssey::kernel::resolver::{ResolvedBinding, ResolvedPlan};
    use odyssey::plugins::test_only::agent::handler_from_plan;

    let (space, factory) = crate::common::boot();
    let m = MB::new("counter", "counter", "counter")
        .in_type("object")
        .out_type("object")
        .action("read", "READ")
        .action("increment", "WRITE")
        .action("reset", "ADMIN")
        .build();
    let slot = factory.mint::<CounterResource>(
        CapKind::Sync,
        &m.exposes[0],
        &m.plugin,
        CapabilityBudget::new(TIMEOUT_MS),
        counter_handler(),
    );

    let consumer = PluginId {
        name: "agent".into(),
        version: "0.1.0".into(),
    };
    let bindings = vec![ResolvedBinding {
        handle: "counter".into(),
        provider: m.plugin.clone(),
        capability: "counter".into(),
        contract: "counter".into(),
    }];
    let plan = ResolvedPlan {
        mint_order: vec![m.plugin.clone(), consumer.clone()],
        bindings: std::iter::once((consumer.clone(), bindings)).collect(),
    };

    let agent =
        handler_from_plan("agent_zeta_10".to_string(), &plan, &consumer, space.clone());

    // The authority vocabulary must include "read" → "READ".
    let cap = space.lookup_by_name("counter").expect("counter cap exists");
    assert_eq!(cap.meta().authority.operation_for("read"), Some("READ"));

    // The agent dispatches "read" successfully because the
    // authority vocabulary says "READ" and the cap holds READ.
    let result = agent
        .invoke(json!({"target": "counter", "op": "read"}))
        .expect("dispatch succeeds");
    assert_eq!(result["target"], "counter");
    assert_eq!(result["action"], "read");
}

// =========================================================================
// ζ.11 — ManifestBuilder::protocol roundtrips
// =========================================================================

#[test]
fn manifest_builder_protocol_roundtrips() {
    // Same builder, with and without protocol metadata. Both
    // build valid manifests; only the protocol differs.
    let m_no = MB::new("counter", "counter", "counter")
        .in_type("object")
        .out_type("object")
        .action("read", "READ")
        .build();
    let m_with = MB::new("counter", "counter", "counter")
        .in_type("object")
        .out_type("object")
        .action("read", "READ")
        .protocol(
            Protocol::empty()
                .with_description("With protocol")
                .with_media_type("application/json")
                .with_version("1.0")
                .with_transport("in-process"),
        )
        .build();

    // Both manifests declare the same authority contract.
    assert_eq!(m_no.exposes[0].authority.actions.len(), 1);
    assert_eq!(m_with.exposes[0].authority.actions.len(), 1);

    // Protocol is empty on the no-protocol build, populated on
    // the with-protocol build.
    assert_eq!(m_no.exposes[0].protocol.description, "");
    assert_eq!(m_with.exposes[0].protocol.description, "With protocol");
    assert_eq!(m_with.exposes[0].protocol.media_type, "application/json");
    assert_eq!(m_with.exposes[0].protocol.version, "1.0");
    assert_eq!(m_with.exposes[0].protocol.transport, "in-process");

    // Cap name and contract_name still propagate.
    assert_eq!(m_no.exposes[0].name, "counter");
    assert_eq!(m_with.exposes[0].contract_name, "counter");
}

// =========================================================================
// Helpers
// =========================================================================

/// Mint a counter cap with a custom protocol block (but default
/// authority — read/increment/reset). Returns the slot id.
fn mint_counter_with_protocol(
    factory: &CapabilityFactory,
    cap_name: &str,
    protocol: Protocol,
) -> odyssey::capability::SlotId {
    let decl = odyssey::kernel::manifest::CapabilityDecl {
        name: cap_name.into(),
        in_type: "object".into(),
        out_type: "object".into(),
        streaming: false,
        authority: AuthorityContract::empty()
            .with_action("read", "READ")
            .with_action("increment", "WRITE")
            .with_action("reset", "ADMIN"),
        protocol,
        ..Default::default()
    };
    factory.mint::<CounterResource>(
        CapKind::Sync,
        &decl,
        &odyssey::kernel::manifest::PluginId {
            name: "counter".into(),
            version: "0.1.0".into(),
        },
        CapabilityBudget::new(TIMEOUT_MS),
        counter_handler(),
    )
}

// Suppress unused warnings for the imports that appear in some
// test configurations but not others.
#[allow(dead_code)]
fn _silence(_cap: Arc<Capability<CounterResource>>, _v: Value) {}