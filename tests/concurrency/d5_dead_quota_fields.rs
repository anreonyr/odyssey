//! D5 reproducer: manifest `tokens_per_minute` / `bytes_per_minute`
//! fields are declared but never enforced — and Phase 5 deletes them.
//!
//! Phase 5 deletes `tokens_per_minute` / `bytes_per_minute` from
//! `QuotaSpec`, so the manifest loader must either:
//!   (a) reject manifests that still set them, OR
//!   (b) silently ignore them with a `tracing::warn!`.
//!
//! We pick (b) to avoid breaking operator workflows on upgrade —
//! the field is gone, but an existing manifest still loads. The
//! inverse — the field still present in `QuotaSpec` — fails this
//! test because the loader would otherwise emit the parsed values.

#[test]
fn quota_spec_has_no_token_or_byte_fields() {
    use odyssey::kernel::QuotaSpec;

    // Compile-time check: after Phase 5, QuotaSpec exposes neither
    // `tokens_per_minute` nor `bytes_per_minute` nor their
    // `with_*` builders. We probe the public surface via a
    // reflection-style check: a default QuotaSpec must serialize
    // without either key.
    let q = QuotaSpec::default();
    let serialized = serde_json::to_value(&q).expect("QuotaSpec serializes");

    let obj = serialized.as_object().expect("QuotaSpec is an object");
    assert!(
        !obj.contains_key("tokens_per_minute"),
        "PRE-PHASE-5 BUG (D5): QuotaSpec still exposes tokens_per_minute"
    );
    assert!(
        !obj.contains_key("bytes_per_minute"),
        "PRE-PHASE-5 BUG (D5): QuotaSpec still exposes bytes_per_minute"
    );
}

#[test]
fn manifest_with_dead_quota_fields_still_loads() {
    use odyssey::host::manifest::PluginManifest;

    let toml_src = r#"
[plugin]
name = "fake"
version = "0.1.0"

[isolate]
kind = "in_proc"

[[exposes]]
name = "cap"
in_type = "any"
out_type = "any"
streaming = false
contract = "echo"
[exposes.quota]
calls_per_minute = 60
tokens_per_minute = 100
bytes_per_minute = 1000
"#;

    // Loading must succeed even when the dead fields are set;
    // the loader either ignores them or warns. We don't assert
    // on the warning — just on the parse succeeding.
    let _ = PluginManifest::from_toml_str(toml_src)
        .expect("manifest loads even with dead quota fields (loader must skip with warn)");
}

/// Phase 5 P2-P: assert that the loader emits the per-field
/// diagnostic. The loader returns a `Vec<String>` of warning
/// lines via `removed_quota_field_warnings`; the public
/// `from_toml_str` prints each via `eprintln!`. We exercise
/// the warning builder directly so the test doesn't have to
/// capture stderr (which the standard test harness doesn't
/// support cleanly).
#[test]
fn d5_warning_capture_tokens_field_fires() {
    use odyssey::host::manifest::load::removed_quota_field_warnings;

    let toml = r#"
[exposes.quota]
calls_per_minute = 60
tokens_per_minute = 100
"#;
    let warnings = removed_quota_field_warnings(toml);
    assert!(
        warnings.iter().any(|w| w.contains("tokens_per_minute")),
        "expected a warning containing 'tokens_per_minute'; got {warnings:?}"
    );
    assert!(
        !warnings.iter().any(|w| w.contains("bytes_per_minute")),
        "manifest without bytes_per_minute must NOT trigger that warning; got {warnings:?}"
    );
}

#[test]
fn d5_warning_capture_bytes_field_fires() {
    use odyssey::host::manifest::load::removed_quota_field_warnings;

    let toml = r#"
[exposes.quota]
calls_per_minute = 60
bytes_per_minute = 1000
"#;
    let warnings = removed_quota_field_warnings(toml);
    assert!(
        warnings.iter().any(|w| w.contains("bytes_per_minute")),
        "expected a warning containing 'bytes_per_minute'; got {warnings:?}"
    );
    assert!(
        !warnings.iter().any(|w| w.contains("tokens_per_minute")),
        "manifest without tokens_per_minute must NOT trigger that warning; got {warnings:?}"
    );
}

#[test]
fn d5_warning_capture_both_fields_fire() {
    use odyssey::host::manifest::load::removed_quota_field_warnings;

    let toml = r#"
[exposes.quota]
calls_per_minute = 60
tokens_per_minute = 100
bytes_per_minute = 1000
"#;
    let warnings = removed_quota_field_warnings(toml);
    assert_eq!(
        warnings.len(),
        2,
        "expected two warnings (tokens + bytes); got {warnings:?}"
    );
    assert!(warnings.iter().any(|w| w.contains("tokens_per_minute")));
    assert!(warnings.iter().any(|w| w.contains("bytes_per_minute")));
}

#[test]
fn d5_warning_capture_no_dead_fields_no_warnings() {
    use odyssey::host::manifest::load::removed_quota_field_warnings;

    let toml = r#"
[exposes.quota]
calls_per_minute = 60
"#;
    let warnings = removed_quota_field_warnings(toml);
    assert!(
        warnings.is_empty(),
        "manifest without removed fields must produce no warnings; got {warnings:?}"
    );
}
