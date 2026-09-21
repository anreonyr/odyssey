//! Generator stub plugin.
//!
//! Backend trait `Generator` + a single stub impl that returns a
//! deterministic placeholder response. Real backends (OpenAI, etc.)
//! can be added as additional `impl Generator for X` blocks.
//!
//! Cap: `generator`. One cap per plugin; plugin name == cap name.

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
use serde_json::{Value, json};

/// Provider abstraction. Future real backends (OpenAI-compatible
/// HTTP, local llama.cpp, etc.) implement this trait.
pub trait Generator: Send + Sync {
    fn complete(&self, prompt: &str) -> Result<String, String>;
}

/// Stub impl: returns a deterministic "[stub] ..." echo. No
/// network, no state. Real implementations replace this.
pub struct StubGenerator;

impl Generator for StubGenerator {
    fn complete(&self, prompt: &str) -> Result<String, String> {
        Ok(format!("[stub generator] prompt len={}", prompt.len()))
    }
}

pub struct GeneratorResource {
    backend: Arc<dyn Generator>,
}

impl Resource for GeneratorResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let prompt = input
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                format!(
                    "generator: expected {{\"prompt\": \"<string>\"}}, got {}",
                    input
                )
            })?;
        let text = self.backend.complete(prompt)?;
        Ok(json!({ "text": text }))
    }
}

pub struct GeneratorBuiltin;

impl BuiltinManifest for GeneratorBuiltin {
    type Resource = GeneratorResource;
    fn manifest(&self) -> PluginManifest {
        ManifestBuilder::new("generator")
            .expose("generator", "generator")
            .timeout_ms(5000)
            .build()
    }
}

impl GeneratorBuiltin {
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
        let backend: Arc<dyn Generator> = Arc::new(StubGenerator);
        let local_slot = pc.mint(
            kind,
            decl,
            budget.clone(),
            Arc::new(GeneratorResource { backend }),
        );
        let rights = CapabilityRights {
            operations: Rights::INVOKE | Rights::ASSIGN,
            timeout_ms: budget.timeout_ms(),
        };
        pc.inner()
            .grant_to::<GeneratorResource>(local_slot, factory.space(), rights, decl.name.clone())
            .map_err(|e| MintError::GrantFailed {
                plugin: plugin.name.clone(),
                cap: decl.name.clone(),
                source: e,
            })
    }

    pub fn register() -> (PluginManifest, MintFn, RuinFn) {
        (
            GeneratorBuiltin.manifest(),
            |factory, plugin, decl, kind, budget, bindings, typed_bindings| {
                GeneratorBuiltin.mint(
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
