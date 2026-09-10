//! Manifest loader — parses TOML into [`PluginManifest`].
//!
//! Phase 5 D5: the loader explicitly detects the removed
//! (removed) quota fields (`tokens_per_minute`,
//! `bytes_per_minute`) and emits a warning so operators on
//! manifests that carried those fields see a diagnostic instead
//! of a silent field drop. The fields are then ignored —
//! serde's default `deny_unknown_fields = false` means
//! `from_str` accepts the rest of the document unchanged.
//!
//! The pre-parse detection is a substring scan, not a proper
//! TOML parse: we only want to flag the *presence* of the
//! removed keys, not enforce their values. False positives
//! (e.g. a comment mentioning the field name) are acceptable —
//! the warning is informational, not an error.

use std::path::Path;

use super::types::{ManifestError, PluginManifest};

/// Parse a TOML string into a [`PluginManifest`].
///
/// Phase 5 D5: emits a warning if the source contains the
/// removed `tokens_per_minute` or `bytes_per_minute` quota
/// fields. The fields are ignored on the new `QuotaSpec`.
pub fn from_toml_str(s: &str) -> Result<PluginManifest, ManifestError> {
    for warning in removed_quota_field_warnings(s) {
        eprintln!("{warning}");
    }
    let m: PluginManifest = toml::from_str(s)?;
    m.validate()?;
    Ok(m)
}

/// Load a manifest from a TOML file on disk.
pub fn from_path(p: impl AsRef<Path>) -> Result<PluginManifest, ManifestError> {
    let text = std::fs::read_to_string(p)?;
    from_toml_str(&text)
}

/// Scan the TOML source for the removed quota fields and return
/// the warning strings. `from_toml_str` prints each via
/// `eprintln!`; tests call this function directly to assert
/// that the right warning fired without capturing stderr.
///
/// The check is intentionally permissive: any line mentioning
/// either key, in any context, triggers the warning. Operators
/// reading the warning then check whether their manifest
/// genuinely carried the field or whether the comment
/// mentioning the field was just historical context.
#[doc(hidden)]
pub fn removed_quota_field_warnings(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if s.contains("tokens_per_minute") {
        out.push(
            "[manifest] WARN: source contains the removed `tokens_per_minute` quota \
             field; Phase 5 removed it from QuotaSpec — limit is dropped"
                .to_string(),
        );
    }
    if s.contains("bytes_per_minute") {
        out.push(
            "[manifest] WARN: source contains the removed `bytes_per_minute` quota \
             field; Phase 5 removed it from QuotaSpec — limit is dropped"
                .to_string(),
        );
    }
    out
}

impl PluginManifest {
    /// Convenience re-export so existing call sites
    /// (`PluginManifest::from_toml_str`, `from_path`) keep
    /// compiling after the loader moves into its own module.
    pub fn from_toml_str(s: &str) -> Result<Self, ManifestError> {
        from_toml_str(s)
    }

    pub fn from_path(p: impl AsRef<Path>) -> Result<Self, ManifestError> {
        from_path(p)
    }
}
