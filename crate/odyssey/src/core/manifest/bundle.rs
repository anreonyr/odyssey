//! Bundle identity — `(name, version)` pair tagging a collection of
//! plugin manifests.
//!
//! Mirrors [`crate::core::identity::ids::PluginId`] in shape: same
//! `(String, String)` fields, same `derive(Ord, PartialOrd, Eq, Hash)`
//! field-wise ordering, same serde derives. The deliberate
//! isomorphism keeps bundle identity composable with plugin identity
//! in renders and error attribution without a separate dispatch
//! table.
//!
//! Identity is recorded on each member manifest as
//! `PluginManifest.bundle: Option<BundleId>` (see
//! `crate::core::manifest::manifest::PluginManifest`). The kernel
//! does NOT use bundle identity for dispatch — `plugin_registry` at
//! `crate::personality::lifecycle::run::plugin_registry` still keys
//! by `plugin.name` — so adding bundles does not introduce a
//! parallel map. The bundle identity exists to (a) group the
//! resolver's `mint_order` in boot diagrams (see
//! `ResolvedPlan::render`) and (b) attribute `ResolveError`
//! variants to the bundle that contained the offending plugin.

use serde::{Deserialize, Serialize};

/// A bundle's stable identity.
///
/// Identity is `(name, version)` — same shape as `PluginId`. The
/// default version is `"0.1.0"` to match `ManifestBuilder::new`'s
/// default for plugins; bundle authors may override via the
/// `ManifestBuilder::bundle(name, version)` setter.
///
/// `Ord`/`PartialOrd`/`Hash` derive field-wise on `(name, version)`
/// — same as `PluginId`. This is what lets a `BTreeMap<BundleId,
/// Vec<PluginId>>` (the structure `ResolvedPlan::render` builds for
/// the bundle-grouped output) iterate in lex order without an
/// explicit comparator.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BundleId {
    pub name: String,
    pub version: String,
}

impl BundleId {
    /// Default version `"0.1.0"` matches `ManifestBuilder::new`'s
    /// plugin default. Bundle authors who care about the version
    /// override via `ManifestBuilder::bundle(name, version)` with
    /// an explicit version string.
    pub const DEFAULT_VERSION: &'static str = "0.1.0";

    /// Construct a bundle id from `(name, version)`.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }

    /// Construct a bundle id using the default version. Equivalent
    /// to `BundleId::new(name, BundleId::DEFAULT_VERSION)`.
    pub fn with_default_version(name: impl Into<String>) -> Self {
        Self::new(name, Self::DEFAULT_VERSION)
    }

    /// Human-readable rendering: `"<name>@<version>"` — same shape
    /// as `PluginId`'s render convention. Used by
    /// `ResolvedPlan::render` and the `ResolveError` `Display`
    /// impls when attributing failures to a bundle.
    pub fn render(&self) -> String {
        format!("{}@{}", self.name, self.version)
    }
}

impl std::fmt::Display for BundleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.render())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_version_matches_plugin_default() {
        // PluginManifest::default version is "0.1.0"
        // (manifest.rs:179). BundleId::DEFAULT_VERSION must match
        // so `BundleId::with_default_version("foo")` produces the
        // same identity string as a builtin that doesn't override.
        assert_eq!(BundleId::DEFAULT_VERSION, "0.1.0");
    }

    #[test]
    fn render_is_name_at_version() {
        let b = BundleId::new("agent-toolkit", "1.2.3");
        assert_eq!(b.render(), "agent-toolkit@1.2.3");
    }

    #[test]
    fn ord_is_field_wise_name_then_version() {
        // name-major: a < b regardless of version
        assert!(BundleId::new("a", "9.9.9") < BundleId::new("b", "0.0.0"));
        // tie on name -> version
        assert!(BundleId::new("a", "0.1.0") < BundleId::new("a", "0.2.0"));
        // equal
        assert_eq!(
            BundleId::new("a", "0.1.0"),
            BundleId::new("a", "0.1.0")
        );
    }

    #[test]
    fn with_default_version_uses_default() {
        let b = BundleId::with_default_version("builtins");
        assert_eq!(b.version, BundleId::DEFAULT_VERSION);
    }
}
