//! γ.5 — End-to-end contract chain.
//!
//! TOML → `PluginManifest` → `CapabilityDecl` → factory.mint →
//! `CapabilityMeta` → `slot_meta(slot)`. The contract that lives on
//! disk is the contract that's observable on the slot.

use odyssey::kernel::manifest::PluginManifest;

#[test]
fn toml_contract_reaches_capability_meta() {
    let toml_src = std::fs::read_to_string("src/plugins/counter/counter.toml")
        .expect("counter.toml present");
    let m = PluginManifest::from_toml_str(&toml_src).expect("counter.toml parses");
    let cap = &m.exposes[0];

    let (space, factory) = crate::common::boot();
    let slot = factory.mint::<odyssey::plugins::counter::CounterResource>(
        odyssey::capability::CapKind::Sync,
        cap,
        &m.plugin,
        odyssey::capability::CapabilityBudget::new(crate::common::DEFAULT_TIMEOUT_MS),
        odyssey::plugins::counter::handler(),
    );
    let observed = space.slot_meta(slot).unwrap();
    assert_eq!(
        observed.contract.description,
        "Shared integer behind a mutex. Three actions: read, increment, reset."
    );
    assert_eq!(
        observed.contract.input_schema["properties"]["op"]["enum"],
        serde_json::json!(["read", "increment", "reset"])
    );
}