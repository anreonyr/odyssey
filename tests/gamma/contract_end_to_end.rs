//! γ.5 — End-to-end protocol/authority chain.
//!
//! Phase 3 P3.2 — TOML → `PluginManifest` → `CapabilityDecl` →
//! factory.mint → `CapabilityMeta` → `slot_meta(slot)`. The
//! protocol metadata and authority action table that live on
//! disk are what's observable on the slot.

use odyssey::host::manifest::PluginManifest;

#[test]
fn toml_protocol_reaches_capability_meta() {
    let toml_src = std::fs::read_to_string("src/plugins/test_only/counter/counter.toml")
        .expect("counter.toml present");
    let m = PluginManifest::from_toml_str(&toml_src).expect("counter.toml parses");
    let cap = &m.exposes[0];

    let (space, factory) = crate::common::boot();
    let slot = factory.mint::<odyssey::plugins::test_only::counter::CounterResource>(
        odyssey::kernel::CapKind::Sync,
        cap,
        &m.plugin,
        odyssey::kernel::CapabilityBudget::new(crate::common::DEFAULT_TIMEOUT_MS),
        odyssey::plugins::test_only::counter::handler(),
    );
    let observed = space.slot_meta(slot).unwrap();
    // Phase 3 P3.2 — the wire-format description and input
    // schema ride through from disk all the way to the slot
    // meta, via the `protocol` field.
    assert_eq!(
        observed.protocol.description,
        "Shared integer behind a mutex. Three actions: read, increment, reset."
    );
    assert_eq!(
        observed.protocol.input_schema["properties"]["op"]["enum"],
        serde_json::json!(["read", "increment", "reset"])
    );
    assert_eq!(observed.authority.actions.len(), 3);
    assert_eq!(observed.authority.operation_for("reset"), Some("ADMIN"));
}