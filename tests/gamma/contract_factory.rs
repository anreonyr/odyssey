//! γ.3 — Protocol metadata forwarded by the factory.
//!
//! Phase 3 P3.2 — A `CapabilityDecl` with a populated
//! `protocol` block reaches `slot.meta().protocol`. The old
//! test name `contract_factory` is preserved so the test id
//! (`γ.3`) keeps its semantic.

use odyssey::kernel::Protocol;
use serde_json::json;

#[test]
fn factory_forwards_decl_protocol() {
    let (space, factory) = crate::common::boot();
    let decl = odyssey::host::manifest::CapabilityDecl {
        name: "demo".into(),
        in_type: "object".into(),
        out_type: "object".into(),
        streaming: false,
        protocol: Protocol::empty()
            .with_description("Phase 3 P3.2 protocol demo")
            .with_input(json!({"type": "object"}))
            .with_media_type("application/json")
            .with_version("1.0")
            .with_transport("in-process"),
        ..Default::default()
    };
    let slot = factory.mint::<odyssey::plugins::test_only::counter::CounterResource>(
        odyssey::kernel::CapKind::Sync,
        &decl,
        &odyssey::kernel::PluginId {
            name: "demo".into(),
            version: "0.1.0".into(),
        },
        odyssey::kernel::CapabilityBudget::new(crate::common::DEFAULT_TIMEOUT_MS),
        odyssey::plugins::test_only::counter::handler(),
    );
    // Phase 3 P3.2 — `slot.meta().protocol` carries the
    // wire-format metadata; `slot.meta().authority` carries
    // the action vocabulary. Both forwarded by the factory.
    let observed = space.slot_meta(slot).unwrap();
    assert_eq!(observed.protocol.description, "Phase 3 P3.2 protocol demo");
    assert_eq!(observed.protocol.input_schema, json!({"type": "object"}));
    assert_eq!(observed.protocol.media_type, "application/json");
    assert_eq!(observed.protocol.version, "1.0");
    assert_eq!(observed.protocol.transport, "in-process");
}