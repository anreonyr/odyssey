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
use odyssey::personality::composition::resolve::ResolvedBinding;
use odyssey::personality::lifecycle::mint::CapabilityFactory;
use odyssey::personality::lifecycle::run::{MintFn, RuinFn, default_ruin};
use serde_json::{Value, json};

/// Reverse resource — `invoke` reverses the `text` field of its
/// input. Domain error on malformed input.
pub struct ReverseResource;

impl Resource for ReverseResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let text = input.get("text").and_then(|v| v.as_str()).ok_or_else(|| {
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
        let tool_schema = serde_json::json!({
            "description": "Reverses the input string character by character. Useful for palindrome checks and string processing.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "The string to reverse. Unicode scalar values are reversed as a sequence, not grapheme clusters."
                    }
                },
                "required": ["text"]
            },
            "output_schema": {
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "The reversed string."
                    }
                },
                "required": ["text"]
            }
        });
        ManifestBuilder::new("reverse")
            .expose_with_schema("reverse", "reverse", tool_schema)
            .timeout_ms(5000)
            .build()
    }
}

impl ReverseBuiltin {
    /// Typed mint — Slice 3 demonstration: reverse's only cap
    /// lives in the plugin's own `PluginCspace`. We then
    /// grant a derived slot into the orchestrator's global
    /// cspace so the HTTP bridge can still look it up by
    /// name. The returned `SlotId` is the GLOBAL one — the
    /// orchestrator's existing teardown path
    /// (`default_ruin` revoking the returned ids) works
    /// unchanged.
    pub fn mint(
        &self,
        factory: &CapabilityFactory,
        plugin: &PluginId,
        decl: &CapabilityDecl,
        kind: CapKind,
        budget: CapabilityBudget,
        _bindings: &[ResolvedBinding],
    ) -> SlotId {
        use odyssey::core::rights::rights::{CapabilityRights, OperationRights};

        let pc = factory.plugin_cspace(plugin);
        let local_slot = pc.mint(kind, decl, budget.clone(), Arc::new(ReverseResource));
        let rights = CapabilityRights {
            operations: OperationRights::ALL,
            timeout_ms: budget.timeout_ms(),
        };
        pc.inner()
            .grant_to::<ReverseResource>(local_slot, factory.space(), rights, decl.name.clone())
            .expect("grant from plugin cspace to global should succeed")
    }

    /// Phase 11: colocated registration helper. Returns the
    /// `(manifest, mint_fn, ruin_fn)` triple; see
    /// `builtins/src/echo.rs::register` for rationale.
    pub fn register() -> (PluginManifest, MintFn, RuinFn) {
        (
            ReverseBuiltin.manifest(),
            |factory, plugin, decl, kind, budget, bindings| {
                ReverseBuiltin.mint(factory, plugin, decl, kind, budget, bindings)
            },
            default_ruin,
        )
    }
}
