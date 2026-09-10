//! γ.2 — Protocol metadata roundtrips through TOML.
//!
//! Phase 3 P3.2 split the old `[exposes.contract]` block into
//! `[exposes.protocol]` (schemas, description, media_type,
//! version, transport) and `[[exposes.authority.actions]]`
//! (action → OperationRights map). The wire-format metadata
//! now lives under `protocol`; the action vocabulary under
//! `authority`.

use odyssey::host::manifest::PluginManifest;

#[test]
fn toml_protocol_parses_into_decl() {
    let toml_src = std::fs::read_to_string("src/plugins/test_only/counter/counter.toml")
        .expect("counter.toml present");
    let m = PluginManifest::from_toml_str(&toml_src).expect("counter.toml parses");
    let cap = &m.exposes[0];
    // Phase 3 P3.2 — protocol metadata lives under `protocol`,
    // not `contract`. Description and schemas moved here.
    assert_eq!(
        cap.protocol.description,
        "Shared integer behind a mutex. Three actions: read, increment, reset."
    );
    assert_eq!(
        cap.protocol.input_schema["properties"]["op"]["enum"],
        serde_json::json!(["read", "increment", "reset"])
    );
    assert_eq!(cap.protocol.media_type, "application/json");
    assert_eq!(cap.protocol.version, "1.0");
    assert_eq!(cap.protocol.transport, "in-process");
}