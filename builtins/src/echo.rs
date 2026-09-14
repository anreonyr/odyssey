//! Echo builtin — pass-through capability.
//!
//! Returns its input verbatim. The simplest capability possible;
//! useful as a smoke test for the resolve + mint + dispatch
//! pipeline.

use std::sync::Arc;

use odyssey::capability::resource::Resource;
use odyssey::core::identity::ids::PluginId;
use odyssey::core::manifest::manifest::{CapabilityDecl, ManifestBuilder, PluginManifest};
use serde_json::Value;

/// Echo resource — `invoke` returns its input unchanged.
pub struct EchoResource;

impl Resource for EchoResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        Ok(input)
    }
}

/// Manifest for the echo plugin. The personality layer feeds
/// this into the resolver.
pub fn manifest() -> PluginManifest {
    ManifestBuilder::new("echo", "echo", "echo")
        .host("dispatcher")
        .action("echo", "EXECUTE")
        .timeout_ms(5000)
        .build()
}

/// Construct the typed echo handler. Returns an `Arc<EchoResource>`
/// so the personality factory can wrap it in a
/// `Capability<EchoResource>`.
pub fn mint(_plugin: &PluginId, _decl: &CapabilityDecl) -> Arc<EchoResource> {
    Arc::new(EchoResource)
}
