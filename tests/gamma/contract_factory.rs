//! γ.3 — Contract forwarded by the factory.
//!
//! A `CapabilityDecl` with a non-empty contract reaches
//! `slot.meta().contract`.

use odyssey::capability::CapabilityContract;
use serde_json::json;

#[test]
fn factory_forwards_decl_contract() {
    let (space, factory) = crate::common::boot();
    let decl = odyssey::kernel::manifest::CapabilityDecl {
        name: "demo".into(),
        in_type: "object".into(),
        out_type: "object".into(),
        streaming: false,
        contract: CapabilityContract::empty()
            .with_description("Phase 2 contract demo")
            .with_input(json!({"type": "object"})),
        ..Default::default()
    };
    let slot = factory.mint::<odyssey::plugins::counter::CounterResource>(
        odyssey::capability::CapKind::Sync,
        &decl,
        &odyssey::kernel::manifest::PluginId {
            name: "demo".into(),
            version: "0.1.0".into(),
        },
        odyssey::capability::CapabilityBudget::new(crate::common::DEFAULT_TIMEOUT_MS),
        odyssey::plugins::counter::handler(),
    );
    let observed = space.slot_meta(slot).unwrap();
    assert_eq!(observed.contract.description, "Phase 2 contract demo");
    assert_eq!(observed.contract.input_schema, json!({"type": "object"}));
}