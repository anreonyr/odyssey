//! γ.4 — Protocol metadata defaults when manifest omits it.
//!
//! Phase 3 P3.2 — missing `[exposes.protocol]` and
//! `[exposes.authority]` both fall back to empty structs.

use serde_json::Value;

#[test]
fn missing_protocol_defaults_to_empty() {
    let (space, factory) = crate::common::boot();
    let decl = odyssey::host::manifest::CapabilityDecl {
        name: "demo".into(),
        in_type: "any".into(),
        out_type: "any".into(),
        streaming: false,
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
    let observed = space.slot_meta(slot).unwrap();
    // Phase 3 P3.2 — both `protocol` and `authority` default
    // to empty. Description is "", input_schema is Null, no
    // actions published.
    assert_eq!(observed.protocol.description, "");
    assert_eq!(observed.protocol.input_schema, Value::Null);
    assert_eq!(observed.protocol.media_type, "");
    assert_eq!(observed.protocol.version, "");
    assert_eq!(observed.protocol.transport, "");
    assert!(observed.authority.actions.is_empty());
}