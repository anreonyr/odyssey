//! Manifest loader tests (Slice 5 of Direction A).
//!
//! Verifies the parsing + validation + file-loading path
//! that `load_plugin_from_path` builds on. Tests live in
//! `crate/odyssey/tests/` (the kernel's own test layer)
//! because they exercise the manifest data shape and
//! validation, not the orchestrator's runtime integration.

use std::io::Write;

use odyssey::core::manifest::manifest::{ManifestInvalid, ManifestLoadError, PluginManifest};

const VALID_JSON: &str = r#"{
    "plugin": {
        "name": "demo_plugin",
        "version": "0.1.0"
    },
    "exposes": [
        {
            "name": "demo_cap",
            "kind": "sync",
            "contract_name": "demo_contract"
        }
    ]
}"#;

const VALID_JSON_WITH_REQUIRES: &str = r#"{
    "plugin": {
        "name": "demo_plugin",
        "version": "0.1.0"
    },
    "exposes": [
        {
            "name": "demo_cap",
            "kind": "sync",
            "contract_name": "demo_contract"
        }
    ],
    "requires": [
        {
            "name": "embedder",
            "contract": "embedder"
        }
    ],
    "timeout_ms": 30000
}"#;

// -----------------------------------------------------------------------
// Happy path
// -----------------------------------------------------------------------

#[test]
fn from_json_str_parses_valid_manifest() {
    let m = PluginManifest::from_json_str(VALID_JSON).expect("valid manifest should parse");
    assert_eq!(m.plugin.name, "demo_plugin");
    assert_eq!(m.plugin.version, "0.1.0");
    assert_eq!(m.exposes.len(), 1);
    assert_eq!(m.exposes[0].name, "demo_cap");
    assert_eq!(m.exposes[0].contract_name, "demo_contract");
    assert!(m.requires.is_empty());
    assert!(m.timeout_ms.is_none());
}

#[test]
fn from_json_str_parses_manifest_with_requires_and_timeout() {
    let m = PluginManifest::from_json_str(VALID_JSON_WITH_REQUIRES)
        .expect("manifest with requires + timeout should parse");
    assert_eq!(m.requires.len(), 1);
    assert_eq!(m.requires[0].name, "embedder");
    assert_eq!(m.requires[0].contract, "embedder");
    assert_eq!(m.timeout_ms, Some(30000));
}

// -----------------------------------------------------------------------
// Parse errors
// -----------------------------------------------------------------------

#[test]
fn from_json_str_rejects_invalid_json() {
    let err = PluginManifest::from_json_str("{ not json").expect_err("invalid JSON should fail");
    assert!(
        matches!(err, ManifestLoadError::Parse(_)),
        "expected Parse error, got {err:?}"
    );
}

#[test]
fn from_json_str_rejects_missing_exposes_via_validation() {
    // `exposes` is `#[serde(default)]` on the struct, so
    // a missing field parses as an empty Vec — the loader's
    // validation catches it as `NoExposes`. (Whether this
    // should be a parse error or a validation error is a
    // schema-design choice; the current path treats empty
    // exposes as `Invalid`.)
    let err = PluginManifest::from_json_str(r#"{"plugin":{"name":"x","version":"0.1.0"}}"#)
        .expect_err("missing exposes should fail");
    assert!(
        matches!(err, ManifestLoadError::Invalid(ref msg) if msg.contains("exposes no capabilities")),
        "missing exposes should fail with NoExposes validation, got {err:?}"
    );
}

// -----------------------------------------------------------------------
// Validation errors
// -----------------------------------------------------------------------

#[test]
fn validate_rejects_empty_plugin_name() {
    let err = PluginManifest::from_json_str(
        r#"{"plugin":{"name":"","version":"0.1.0"},"exposes":[{"name":"c","kind":"sync","contract_name":"k"}]}"#,
    )
    .expect_err("empty plugin.name should fail validation");
    assert!(
        matches!(err, ManifestLoadError::Invalid(ref msg) if msg.contains("plugin.name is empty")),
        "expected empty-name validation error, got {err:?}"
    );
}

#[test]
fn validate_rejects_empty_plugin_version() {
    let err = PluginManifest::from_json_str(
        r#"{"plugin":{"name":"p","version":""},"exposes":[{"name":"c","kind":"sync","contract_name":"k"}]}"#,
    )
    .expect_err("empty plugin.version should fail validation");
    assert!(
        matches!(err, ManifestLoadError::Invalid(ref msg) if msg.contains("plugin.version is empty")),
        "expected empty-version validation error, got {err:?}"
    );
}

#[test]
fn validate_rejects_no_exposes() {
    let err =
        PluginManifest::from_json_str(r#"{"plugin":{"name":"p","version":"0.1.0"},"exposes":[]}"#)
            .expect_err("empty exposes should fail validation");
    assert!(
        matches!(err, ManifestLoadError::Invalid(ref msg) if msg.contains("exposes no capabilities")),
        "expected no-exposes validation error, got {err:?}"
    );
}

#[test]
fn validate_rejects_empty_expose_name() {
    let err = PluginManifest::from_json_str(
        r#"{"plugin":{"name":"p","version":"0.1.0"},"exposes":[{"name":"","kind":"sync","contract_name":"k"}]}"#,
    )
    .expect_err("empty expose name should fail validation");
    assert!(
        matches!(err, ManifestLoadError::Invalid(ref msg) if msg.contains("exposes[0].name is empty")),
        "expected empty-name validation error, got {err:?}"
    );
}

#[test]
fn validate_rejects_empty_expose_contract() {
    let err = PluginManifest::from_json_str(
        r#"{"plugin":{"name":"p","version":"0.1.0"},"exposes":[{"name":"c","kind":"sync","contract_name":""}]}"#,
    )
    .expect_err("empty contract_name should fail validation");
    assert!(
        matches!(err, ManifestLoadError::Invalid(ref msg) if msg.contains("exposes[0].contract_name is empty")),
        "expected empty-contract validation error, got {err:?}"
    );
}

#[test]
fn validate_rejects_duplicate_expose_names() {
    let err = PluginManifest::from_json_str(
        r#"{
            "plugin":{"name":"p","version":"0.1.0"},
            "exposes":[
                {"name":"dup","kind":"sync","contract_name":"a"},
                {"name":"dup","kind":"sync","contract_name":"b"}
            ]
        }"#,
    )
    .expect_err("duplicate expose names should fail validation");
    assert!(
        matches!(err, ManifestLoadError::Invalid(ref msg) if msg.contains("duplicate") && msg.contains("dup")),
        "expected duplicate-name validation error, got {err:?}"
    );
}

#[test]
fn validate_rejects_empty_require_handle() {
    let err = PluginManifest::from_json_str(
        r#"{
            "plugin":{"name":"p","version":"0.1.0"},
            "exposes":[{"name":"c","kind":"sync","contract_name":"k"}],
            "requires":[{"name":"","contract":"x"}]
        }"#,
    )
    .expect_err("empty require handle should fail validation");
    assert!(
        matches!(err, ManifestLoadError::Invalid(ref msg) if msg.contains("requires[0].name is empty")),
        "expected empty-handle validation error, got {err:?}"
    );
}

// -----------------------------------------------------------------------
// File loading
// -----------------------------------------------------------------------

#[test]
fn from_path_loads_valid_manifest_file() {
    // Distinct subdir per test name so cargo's parallel
    // test runner doesn't have two tests racing on the
    // same `/tmp/odyssey-loader-test-{pid}` directory —
    // one would `remove_dir_all` while the other is still
    // reading from it.
    let dir =
        std::env::temp_dir().join(format!("odyssey-loader-test-{}-valid", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join("valid.manifest.json");
    let mut f = std::fs::File::create(&path).expect("create file");
    f.write_all(VALID_JSON.as_bytes()).expect("write");
    drop(f);

    let m = PluginManifest::from_path(&path).expect("valid file should load");
    assert_eq!(m.plugin.name, "demo_plugin");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn from_path_returns_io_error_for_missing_file() {
    let path = std::path::PathBuf::from("/nonexistent/odyssey/does/not/exist.json");
    let err = PluginManifest::from_path(&path).expect_err("missing file should fail");
    assert!(
        matches!(err, ManifestLoadError::Io(_)),
        "expected Io error, got {err:?}"
    );
}

#[test]
fn from_path_validates_after_parsing() {
    // Write a syntactically valid JSON file that fails
    // validation (empty plugin name). The file should load
    // and parse, then validation should reject it.
    // See `from_path_loads_valid_manifest_file` for why
    // this needs its own subdir name.
    let dir = std::env::temp_dir().join(format!(
        "odyssey-loader-test-{}-invalid",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join("invalid.manifest.json");
    let mut f = std::fs::File::create(&path).expect("create file");
    f.write_all(
        br#"{"plugin":{"name":"","version":"0.1.0"},"exposes":[{"name":"c","kind":"sync","contract_name":"k"}]}"#,
    )
    .expect("write");
    drop(f);

    let err = PluginManifest::from_path(&path).expect_err("invalid manifest should fail");
    assert!(
        matches!(err, ManifestLoadError::Invalid(_)),
        "expected Invalid error after parse, got {err:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// -----------------------------------------------------------------------
// Display + Debug sanity
// -----------------------------------------------------------------------

#[test]
fn manifest_invalid_display_contains_field_index() {
    let v = ManifestInvalid::EmptyExposeName { index: 7 };
    let s = v.to_string();
    assert!(s.contains("exposes[7].name"));
}

#[test]
fn manifest_load_error_display_includes_underlying_message() {
    let e = ManifestLoadError::Parse("unexpected token".to_string());
    let s = e.to_string();
    assert!(s.contains("manifest parse"));
    assert!(s.contains("unexpected token"));
}
