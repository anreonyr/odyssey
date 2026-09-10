//! Manifest loader — parses TOML into [`PluginManifest`].
//!
//! Phase 5 removed the legacy `tokens_per_minute` /
//! `bytes_per_minute` quota fields from `QuotaSpec`. The loader
//! relies on serde's default `deny_unknown_fields = false`, so
//! manifests that still set those keys parse unchanged: the
//! unknown fields are silently dropped, no diagnostic is
//! emitted. Tests in `tests/concurrency/d5_dead_quota_fields.rs`
//! guard the schema-level invariant (the keys are gone from
//! `QuotaSpec` and a manifest carrying them still loads).

use std::path::Path;

use super::types::{ManifestError, PluginManifest};

/// Parse a TOML string into a [`PluginManifest`].
pub fn from_toml_str(s: &str) -> Result<PluginManifest, ManifestError> {
    let m: PluginManifest = toml::from_str(s)?;
    m.validate()?;
    Ok(m)
}

/// Load a manifest from a TOML file on disk.
pub fn from_path(p: impl AsRef<Path>) -> Result<PluginManifest, ManifestError> {
    let text = std::fs::read_to_string(p)?;
    from_toml_str(&text)
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
