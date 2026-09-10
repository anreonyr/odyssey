//! γ.6 — Contract action table roundtrips through TOML.
//!
//! The `[[exposes.contract.actions]]` blocks in counter.toml
//! deserialise into `CapabilityContract.actions`, survive the
//! factory's mint, and are observable via the same `slot_meta`
//! path as `input_schema` / `output_schema` / `description`.

use odyssey::capability::CapabilityContract;
use odyssey::kernel::manifest::PluginManifest;

#[test]
fn counter_publishes_three_actions() {
    let toml_src = std::fs::read_to_string("src/plugins/counter/counter.toml")
        .expect("counter.toml present");
    let m = PluginManifest::from_toml_str(&toml_src).expect("counter.toml parses");
    let cap = &m.exposes[0];
    assert_eq!(cap.contract.actions.len(), 3, "counter should publish 3 actions");

    // Each action maps a name to an operation bit string. The exact
    // string is what the agent parses, so this is the public
    // contract surface.
    assert_eq!(cap.contract.operation_for("read"), Some("READ"));
    assert_eq!(cap.contract.operation_for("increment"), Some("WRITE"));
    assert_eq!(cap.contract.operation_for("reset"), Some("ADMIN"));
    assert_eq!(cap.contract.operation_for("nonsense"), None);
}

#[test]
fn factory_preserves_action_table() {
    let m = PluginManifest::from_toml_str(
        &std::fs::read_to_string("src/plugins/counter/counter.toml").unwrap(),
    )
    .unwrap();

    let space = odyssey::capability::CapabilitySpace::new();
    let factory = odyssey::kernel::factory::CapabilityFactory::new(space.clone());
    let slot = factory.mint::<odyssey::plugins::counter::CounterResource>(
        odyssey::capability::CapKind::Sync,
        &m.exposes[0],
        &m.plugin,
        odyssey::capability::CapabilityBudget::new(crate::common::DEFAULT_TIMEOUT_MS),
        odyssey::plugins::counter::handler(),
    );
    let observed = space.slot_meta(slot).unwrap();
    assert_eq!(observed.contract.actions.len(), 3);
    assert_eq!(observed.contract.operation_for("reset"), Some("ADMIN"));
}

#[test]
fn empty_contract_has_no_actions() {
    let c = CapabilityContract::default();
    assert!(c.actions.is_empty());
    assert_eq!(c.operation_for("anything"), None);
}

#[test]
fn with_action_builder_accumulates() {
    let c = CapabilityContract::empty()
        .with_action("read", "READ")
        .with_action("increment", "WRITE")
        .with_action("reset", "ADMIN");
    assert_eq!(c.actions.len(), 3);
    assert_eq!(c.operation_for("read"), Some("READ"));
    assert_eq!(c.operation_for("increment"), Some("WRITE"));
    assert_eq!(c.operation_for("reset"), Some("ADMIN"));
}