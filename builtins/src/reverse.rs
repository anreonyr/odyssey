//! Reverse builtin — string reversal capability.
//!
//! Phase 5 plugin (was `plugins/reverse/`). Demoted to a builtin
//! in Phase 8 because the 11-plugin dispatch table that
//! distinguished each plugin was replaced by a workspace-member
//! model where each builtin lives in its own module.
//!
//! Input is expected to be `{ "text": "..." }`; output is the
//! reversed string. Any input that isn't a JSON object containing
//! a `text` field produces a domain error.

use std::sync::Arc;

use odyssey::capability::enforce::quota::CapabilityBudget;
use odyssey::core::Resource;
use odyssey::core::contract::builtin::BuiltinManifest;
use odyssey::core::identity::ids::{PluginId, SlotId};
use odyssey::core::identity::kind::CapKind;
use odyssey::core::manifest::manifest::{CapabilityDecl, ManifestBuilder, PluginManifest};
use odyssey::personality::lifecycle::mint::CapabilityFactory;
use odyssey::personality::lifecycle::run::MintFn;
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

pub struct ReverseBuiltin;

impl BuiltinManifest for ReverseBuiltin {
    fn manifest(&self) -> PluginManifest {
        ManifestBuilder::new("reverse", "reverse", "reverse")
            .host("dispatcher")
            .timeout_ms(5000)
            .build()
    }
}

impl ReverseBuiltin {
    pub fn mint(
        &self,
        factory: &CapabilityFactory,
        plugin: &PluginId,
        decl: &CapabilityDecl,
        kind: CapKind,
        budget: CapabilityBudget,
    ) -> SlotId {
        factory.mint(kind, decl, plugin, budget, Arc::new(ReverseResource))
    }

    /// Phase 10: colocated registration helper. See
    /// `builtins/src/echo.rs::register` for rationale.
    pub fn register() -> (PluginManifest, MintFn) {
        (
            ReverseBuiltin.manifest(),
            |factory, plugin, decl, kind, budget| {
                ReverseBuiltin.mint(factory, plugin, decl, kind, budget)
            },
        )
    }
}
