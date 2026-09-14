//! Reverse builtin — string reversal capability.
//!
//! Phase 5 plugin (was `plugins/reverse/`). Demoted to a builtin
//! in Phase 8 because the 11-plugin dispatch table that
//! distinguished each plugin was replaced by a workspace-member
//! model where each builtin lives in its own module.
//!
//! Input is expected to be `{ "text": "..." }`; output is the
//! reversed string. Any input that isn't a JSON object containing
//! a `text` field produces a domain error (`Err("reverse: expected
//! {\"text\": \"<string>\"}, got <shape>")`).

use std::sync::Arc;

use odyssey::capability::resource::Resource;
use odyssey::core::identity::ids::PluginId;
use odyssey::core::manifest::manifest::{CapabilityDecl, ManifestBuilder, PluginManifest};
use serde_json::{json, Value};

/// Reverse resource — `invoke` reverses the `text` field of its
/// input. Domain error on malformed input.
pub struct ReverseResource;

impl Resource for ReverseResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let text = input
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                format!(
                    "reverse: expected {{\"text\": \"<string>\"}}, got {}",
                    input
                )
            })?;
        let reversed: String = text.chars().rev().collect();
        Ok(json!({ "text": reversed }))
    }
}

/// Manifest for the reverse plugin.
pub fn manifest() -> PluginManifest {
    ManifestBuilder::new("reverse", "reverse", "reverse")
        .host("dispatcher")
        .action("reverse", "EXECUTE")
        .timeout_ms(5000)
        .build()
}

/// Construct the typed reverse handler.
pub fn mint(_plugin: &PluginId, _decl: &CapabilityDecl) -> Arc<ReverseResource> {
    Arc::new(ReverseResource)
}
