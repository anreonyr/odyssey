//! Reranker stub plugin.
//!
//! Backend trait `Reranker` + a single stub impl that returns
//! the input order unchanged. Real reranking models implement
//! this trait.
//!
//! Cap: `reranker`. One cap per plugin; plugin name == cap name.

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

pub trait Reranker: Send + Sync {
    /// Return indices into `candidates` ordered from most to
    /// least relevant for `query`. The stub returns the input
    /// order; real implementations do the actual ranking.
    fn rerank(&self, query: &str, candidates: Vec<String>) -> Result<Vec<usize>, String>;
}

/// Stub impl: preserves input order. Real backends (Cohere
/// Rerank, local cross-encoders, ...) replace this.
pub struct StubReranker;

impl Reranker for StubReranker {
    fn rerank(&self, _query: &str, candidates: Vec<String>) -> Result<Vec<usize>, String> {
        Ok((0..candidates.len()).collect())
    }
}

pub struct RerankerResource {
    backend: Arc<dyn Reranker>,
}

impl Resource for RerankerResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let query = input.get("query").and_then(|v| v.as_str()).ok_or_else(|| {
            format!(
                "reranker: expected {{\"query\": \"<string>\", \"candidates\": [...]}}, got {}",
                input
            )
        })?;
        let candidates = input
            .get("candidates")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                format!(
                    "reranker: expected {{\"query\": \"<string>\", \"candidates\": [...]}}, got {}",
                    input
                )
            })?
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect::<Vec<_>>();
        let order = self.backend.rerank(query, candidates)?;
        Ok(json!({ "order": order }))
    }
}

pub struct RerankerBuiltin;

impl BuiltinManifest for RerankerBuiltin {
    type Resource = RerankerResource;
    fn manifest(&self) -> PluginManifest {
        ManifestBuilder::new("reranker")
            .expose("reranker", "reranker")
            .timeout_ms(5000)
            .build()
    }
}

impl RerankerBuiltin {
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
        let backend: Arc<dyn Reranker> = Arc::new(StubReranker);
        let local_slot = pc.mint(
            kind,
            decl,
            budget.clone(),
            Arc::new(RerankerResource { backend }),
        );
        let rights = CapabilityRights {
            operations: Rights::INVOKE | Rights::ASSIGN,
            timeout_ms: budget.timeout_ms(),
        };
        pc.inner()
            .grant_to::<RerankerResource>(local_slot, factory.space(), rights, decl.name.clone())
            .map_err(|e| MintError::GrantFailed {
                plugin: plugin.name.clone(),
                cap: decl.name.clone(),
                source: e,
            })
    }

    pub fn register() -> (PluginManifest, MintFn, RuinFn) {
        (
            RerankerBuiltin.manifest(),
            |factory, plugin, decl, kind, budget, bindings, typed_bindings| {
                RerankerBuiltin.mint(
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
