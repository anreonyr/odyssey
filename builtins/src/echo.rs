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
use odyssey::personality::lifecycle::mint::CapabilityFactory;
use odyssey::personality::lifecycle::run::Mint;
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
        ManifestBuilder::new("echo", "echo", "echo")
            .host("dispatcher")
            .action("echo", "EXECUTE")
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
    ) -> Result<SlotId, String> {
        Ok(factory.mint(kind, decl, plugin, budget, Arc::new(EchoResource)))
    }
}

impl Mint for EchoBuiltin {
    fn mint(
        &self,
        factory: &CapabilityFactory,
        plugin: &PluginId,
        decl: &CapabilityDecl,
        kind: CapKind,
        budget: CapabilityBudget,
    ) -> Result<SlotId, String> {
        EchoBuiltin::mint(self, factory, plugin, decl, kind, budget)
    }
}

