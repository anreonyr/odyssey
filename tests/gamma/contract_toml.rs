//! γ.2 — Contract metadata roundtrips through TOML.
//!
//! The `[exposes.contract]` block in a manifest parses into
//! `CapabilityContract` and survives into `CapabilityMeta`.

use odyssey::kernel::manifest::PluginManifest;

#[test]
fn toml_contract_parses_into_decl() {
    let toml_src = std::fs::read_to_string("src/plugins/counter/counter.toml")
        .expect("counter.toml present");
    let m = PluginManifest::from_toml_str(&toml_src).expect("counter.toml parses");
    let cap = &m.exposes[0];
    assert_eq!(
        cap.contract.description,
        "Shared integer behind a mutex. Three actions: read, increment, reset."
    );
    assert_eq!(
        cap.contract.input_schema["properties"]["op"]["enum"],
        serde_json::json!(["read", "increment", "reset"])
    );
}