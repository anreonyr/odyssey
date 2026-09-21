//! Embedder stub plugin.
//!
//! Backend trait `Embedder` + a single stub impl that returns a
//! fixed-length zero vector. Real backends (OpenAI embeddings,
//! sentence-transformers, etc.) implement this trait.
//!
//! Cap: `embedder`. One cap per plugin; plugin name == cap name.

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

pub trait Embedder: Send + Sync {
    fn embed(&self, text: &str) -> Result<Vec<f32>, String>;
}

/// Stub impl: returns a fixed-length zero vector. Real
/// embeddings replace this.
pub struct StubEmbedder;

impl Embedder for StubEmbedder {
    fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
        Ok(vec![0.0; 8 + text.len().min(24)])
    }
}

pub struct EmbedderResource {
    backend: Arc<dyn Embedder>,
}

impl Resource for EmbedderResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let text = input.get("text").and_then(|v| v.as_str()).ok_or_else(|| {
            format!(
                "embedder: expected {{\"text\": \"<string>\"}}, got {}",
                input
            )
        })?;
        let vector = self.backend.embed(text)?;
        Ok(json!({ "vector": vector }))
    }
}

pub struct EmbedderBuiltin;

impl BuiltinManifest for EmbedderBuiltin {
    type Resource = EmbedderResource;
    fn manifest(&self) -> PluginManifest {
        ManifestBuilder::new("embedder")
            .expose("embedder", "embedder")
            .timeout_ms(5000)
            .build()
    }
}

impl EmbedderBuiltin {
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
        let backend: Arc<dyn Embedder> = Arc::new(StubEmbedder);
        let local_slot = pc.mint(
            kind,
            decl,
            budget.clone(),
            Arc::new(EmbedderResource { backend }),
        );
        let rights = CapabilityRights {
            operations: Rights::INVOKE | Rights::ASSIGN,
            timeout_ms: budget.timeout_ms(),
        };
        pc.inner()
            .grant_to::<EmbedderResource>(local_slot, factory.space(), rights, decl.name.clone())
            .map_err(|e| MintError::GrantFailed {
                plugin: plugin.name.clone(),
                cap: decl.name.clone(),
                source: e,
            })
    }

    pub fn register() -> (PluginManifest, MintFn, RuinFn) {
        (
            EmbedderBuiltin.manifest(),
            |factory, plugin, decl, kind, budget, bindings, typed_bindings| {
                EmbedderBuiltin.mint(
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
