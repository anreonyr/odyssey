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
use odyssey::personality::lifecycle::mint::CapabilityFactory;
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
    fn manifest(&self) -> PluginManifest {
        ManifestBuilder::new("echo")
            .expose("echo", "echo")
            .host("dispatcher")
            .timeout_ms(5000)
            .build()
    }
}

impl EchoBuiltin {
    /// Typed mint — calls the factory's generic `mint<EchoResource>`.
    pub fn mint(
        &self,
        factory: &CapabilityFactory,
        plugin: &PluginId,
        decl: &CapabilityDecl,
        kind: CapKind,
        budget: CapabilityBudget,
        _bindings: &[ResolvedBinding],
    ) -> SlotId {
        factory.mint(kind, decl, plugin, budget, Arc::new(EchoResource))
    }

    /// Phase 11: colocated registration helper. Returns the
    /// `(manifest, mint_fn, ruin_fn)` triple the orchestrator's
    /// `run(plugins)` expects. The third element is the
    /// default teardown (just `cspace.revoke_tree` per slot) —
    /// no custom cleanup needed for stateless builtins like
    /// echo.
    pub fn register() -> (PluginManifest, MintFn, RuinFn) {
        (
            EchoBuiltin.manifest(),
            |factory, plugin, decl, kind, budget, bindings| {
                EchoBuiltin.mint(factory, plugin, decl, kind, budget, bindings)
            },
            default_ruin,
        )
    }
}
