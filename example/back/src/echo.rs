//! Echo builtin — pass-through capability.
//!
//! Returns its input verbatim. The simplest capability possible;
//! useful as a smoke test for the resolve + mint + dispatch
//! pipeline.

use std::sync::Arc;

use odyssey::capability::enforce::quota::CapabilityBudget;
use odyssey::core::Resource;
use odyssey::core::contract::builtin::BuiltinManifest;
use odyssey::core::identity::ids::{PluginId, SlotId};
use odyssey::core::identity::kind::CapKind;
use odyssey::core::manifest::manifest::{CapabilityDecl, ManifestBuilder, PluginManifest};
use odyssey::personality::composition::resolve::ResolvedBinding;
use odyssey::personality::lifecycle::mint::{CapabilityFactory, MintError, TypedBindings};
use odyssey::personality::lifecycle::run::{MintFn, RuinFn, default_ruin};
use serde_json::Value;

/// Echo resource — `invoke` returns its input unchanged.
pub struct EchoResource;

impl Resource for EchoResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        Ok(input)
    }
}

/// Concrete echo builtin. Exposes `manifest()` (via the
/// `BuiltinManifest` trait) and `mint()` (typed concrete method
/// that calls the generic factory with `Arc<EchoResource>`).
pub struct EchoBuiltin;

impl BuiltinManifest for EchoBuiltin {
    type Resource = EchoResource;
    fn manifest(&self) -> PluginManifest {
        // `tool_schema` is opaque to the kernel but lets the
        // `tool_descriptor` cap answer "what is this tool's
        // input/output contract?" and lets the agent advertise
        // the tool to a real LLM via native tool calling. Echo
        // takes any JSON input verbatim and returns it
        // verbatim, so the schema is `{"type":"object"}` with
        // a `value` property that takes any shape.
        let tool_schema = serde_json::json!({
            "description": "Echoes its input back unchanged. The agent uses this to verify the tool-call round-trip.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "value": { "description": "Anything JSON-serialisable; returned verbatim." }
                }
            },
            "output_schema": {
                "type": "object",
                "description": "The exact JSON value the caller supplied."
            }
        });
        ManifestBuilder::new("echo")
            .expose_with_schema("echo", "echo", tool_schema)
            .timeout_ms(5000)
            .build()
    }
}

impl EchoBuiltin {
    /// Typed mint — Slice 3 demonstration: echo's only cap
    /// lives in the plugin's own `PluginCspace`. We then
    /// grant a derived slot into the orchestrator's global
    /// cspace so the HTTP bridge can still look it up by
    /// name. The returned `SlotId` is the GLOBAL one — the
    /// orchestrator's existing teardown path
    /// (`default_ruin` revoking the returned ids) works
    /// unchanged.
    ///
    /// DI Phase 21: `typed_bindings` carries the consumer's
    /// pre-resolved typed slot ids. Echo is a leaf (no
    /// `requires`), so this argument is unused; the
    /// underscore-prefixed name documents the intent. The
    /// `grant_to` failure surface is mapped to
    /// `MintError::GrantFailed` so the orchestrator can
    /// surface a typed boot-time error instead of panicking.
    pub fn mint(
        &self,
        factory: &CapabilityFactory,
        plugin: &PluginId,
        decl: &CapabilityDecl,
        kind: CapKind,
        budget: CapabilityBudget,
        _bindings: &[ResolvedBinding],
        _typed_bindings: &TypedBindings,
    ) -> Result<SlotId, MintError> {
        use odyssey::core::rights::rights::{CapabilityRights, Rights};

        let pc = factory.plugin_cspace(plugin);
        let local_slot = pc.mint(kind, decl, budget.clone(), Arc::new(EchoResource));
        let rights = CapabilityRights {
            operations: Rights::INVOKE | Rights::ASSIGN,
            timeout_ms: budget.timeout_ms(),
        };
        pc.inner()
            .grant_to::<EchoResource>(local_slot, factory.space(), rights, decl.name.clone())
            .map_err(|e| MintError::GrantFailed {
                plugin: plugin.name.clone(),
                cap: decl.name.clone(),
                source: e,
            })
    }

    /// Phase 11: colocated registration helper. Returns the
    /// `(manifest, mint_fn, ruin_fn)` triple the orchestrator's
    /// `run(plugins)` expects. The third element is the
    /// default teardown (just `cspace.revoke_tree` per slot) —
    /// no custom cleanup needed for stateless builtins like
    /// echo.
    ///
    /// DI Phase 21: `MintFn` signature widens (typed_bindings
    /// parameter, `Result` return). The closure body forwards
    /// the new argument to the inherent `mint`.
    pub fn register() -> (PluginManifest, MintFn, RuinFn) {
        (
            EchoBuiltin.manifest(),
            |factory, plugin, decl, kind, budget, bindings, typed_bindings| {
                EchoBuiltin.mint(
                    factory,
                    plugin,
                    decl,
                    kind,
                    budget,
                    bindings,
                    typed_bindings,
                )
            },
            default_ruin,
        )
    }
}
