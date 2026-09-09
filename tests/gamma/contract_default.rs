//! γ.4 — Contract defaults when manifest omits it.
//!
//! Missing `[exposes.contract]` falls back to
//! `CapabilityContract::default()`.

use serde_json::Value;

#[test]
fn missing_contract_defaults_to_empty() {
    let (space, factory) = crate::common::boot();
    let decl = odyssey::host::manifest::CapabilityDecl {
        name: "demo".into(),
        in_type: "any".into(),
        out_type: "any".into(),
        streaming: false,
        ..Default::default()
    };
    let slot = factory.mint::<odyssey::plugins::counter::CounterResource>(
        odyssey::capability::CapKind::Sync,
        &decl,
        &odyssey::host::manifest::PluginId {
            name: "demo".into(),
            version: "0.1.0".into(),
        },
        odyssey::capability::CapabilityBudget::new(crate::common::DEFAULT_TIMEOUT_MS),
        odyssey::plugins::counter::handler(),
    );
    let observed = space.slot_meta(slot).unwrap();
    assert_eq!(observed.contract.description, "");
    assert_eq!(observed.contract.input_schema, Value::Null);
}