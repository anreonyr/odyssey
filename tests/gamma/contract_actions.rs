//! γ.6 — Authority action table roundtrips through TOML.
//!
//! Phase 3 P3.2: the `[[exposes.authority.actions]]` blocks in
//! counter.toml deserialise into `AuthorityContract.actions`,
//! survive the factory's mint, and are observable via the same
//! `slot_meta` path as `protocol` (schemas, description).

use odyssey::kernel::AuthorityContract;
use odyssey::host::manifest::PluginManifest;

#[test]
fn counter_publishes_three_actions() {
    let toml_src = std::fs::read_to_string("src/plugins/test_only/counter/counter.toml")
        .expect("counter.toml present");
    let m = PluginManifest::from_toml_str(&toml_src).expect("counter.toml parses");
    let cap = &m.exposes[0];
    // Phase 3 P3.2 — action vocabulary lives under `authority`,
    // not `contract`.
    assert_eq!(cap.authority.actions.len(), 3, "counter should publish 3 actions");

    // Each action maps a name to an operation bit string. The
    // exact string is what the agent parses, so this is the
    // public authority surface.
    assert_eq!(cap.authority.operation_for("read"), Some("READ"));
    assert_eq!(cap.authority.operation_for("increment"), Some("WRITE"));
    assert_eq!(cap.authority.operation_for("reset"), Some("ADMIN"));
    assert_eq!(cap.authority.operation_for("nonsense"), None);
}

#[test]
fn factory_preserves_authority_action_table() {
    let m = PluginManifest::from_toml_str(
        &std::fs::read_to_string("src/plugins/test_only/counter/counter.toml").unwrap(),
    )
    .unwrap();

    let space = odyssey::kernel::CapabilitySpace::new();
    let factory = odyssey::host::factory::CapabilityFactory::new(space.clone());
    let slot = factory.mint::<odyssey::plugins::test_only::counter::CounterResource>(
        odyssey::kernel::CapKind::Sync,
        &m.exposes[0],
        &m.plugin,
        odyssey::kernel::CapabilityBudget::new(crate::common::DEFAULT_TIMEOUT_MS),
        odyssey::plugins::test_only::counter::handler(),
    );
    // Phase 3 P3.2 — `slot.meta()` now exposes two fields:
    // `authority` (load-bearing) and `protocol` (metadata).
    let observed = space.slot_meta(slot).unwrap();
    assert_eq!(observed.authority.actions.len(), 3);
    assert_eq!(observed.authority.operation_for("reset"), Some("ADMIN"));
}

#[test]
fn empty_authority_has_no_actions() {
    let a = AuthorityContract::default();
    assert!(a.actions.is_empty());
    assert_eq!(a.operation_for("anything"), None);
}

#[test]
fn with_action_builder_accumulates() {
    let a = AuthorityContract::empty()
        .with_action("read", "READ")
        .with_action("increment", "WRITE")
        .with_action("reset", "ADMIN");
    assert_eq!(a.actions.len(), 3);
    assert_eq!(a.operation_for("read"), Some("READ"));
    assert_eq!(a.operation_for("increment"), Some("WRITE"));
    assert_eq!(a.operation_for("reset"), Some("ADMIN"));
}