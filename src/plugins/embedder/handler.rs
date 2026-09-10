//! Embedder plugin — `EmbedderResource: Resource`. Sync only.
//!
//! Phase 4 P4.2 — the second AI-shaped capability after
//! `Generator`. Exposes one operation, `embed(text) → Vec<f32>`,
//! behind a single capability slot (`embed`).
//!
//! ## Why this is "AI-shaped" but not really an embedder
//!
//! Phase 4 is about *composition*. The Embedder's job here is
//! to provide the **shape** of an embedding capability:
//!   - input: a text payload
//!   - output: a fixed-dimension vector
//!   - deterministic for the same input
//!
//! The actual vectors are produced by a deterministic byte-
//! rolling-hash projection (`text.bytes()` cycled into `dim`
//! floats, each normalised to `[-1.0, 1.0]`). This is enough
//! for tests that need "same input → same vector" and "different
//! input → different vector"; it is not a semantic embedder.
//!
//! A real embedder (ONNX runtime, sentence-transformers, OpenAI
//! `text-embedding-3-small`) drops in behind the same
//! `invoke()` shape later — the resource is a single file.
//!
//! ## Why this is sync, not streaming
//!
//! `Resource::invoke` is the one-shot (sync) shape. Embedding
//! is a single-call API; the result is a single vector. If a
//! future caller needs progressive partial embeddings (e.g.
//! sentence-by-sentence), that goes through a streaming
//! capability with its own contract.
//!
//! ## Input shape
//!
//! ```json
//! { "text": "the kernel", "dim": 8 }
//! ```
//!
//! `dim` is optional (default 8, max 64). The output is:
//!
//! ```json
//! { "vector": [0.42, -0.17, 0.93, ...] }
//! ```

use std::sync::Arc;

use cordis::{plugin_with, Context, Injection, LogLevel, Plugin};
use serde_json::{json, Value};

use crate::kernel::{Resource, Slot};

pub const DEFAULT_DIM: usize = 8;
pub const MAX_DIM: usize = 64;

pub struct EmbedderResource {
    dim: usize,
}

impl EmbedderResource {
    pub fn new() -> Self {
        Self { dim: DEFAULT_DIM }
    }

    pub fn with_dim(dim: usize) -> Self {
        Self { dim: dim.clamp(1, MAX_DIM) }
    }

    /// The default embedding dimension this resource produces
    /// when the caller doesn't specify one.
    pub fn dim(&self) -> usize {
        self.dim
    }
}

impl Default for EmbedderResource {
    fn default() -> Self {
        Self::new()
    }
}

/// Project a text string into a deterministic `dim`-dimensional
/// vector. Each axis is the sum of byte positions in its window
/// divided by `255`, normalised to `[-1.0, 1.0]`.
///
/// **Not a real embedder** — see module docs. The output is
/// stable for the same input and varies meaningfully across
/// distinct inputs, which is all Phase 4 needs.
fn project(text: &str, dim: usize) -> Vec<f32> {
    let bytes = text.as_bytes();
    let mut v = vec![0.0_f32; dim];
    if bytes.is_empty() {
        return v;
    }
    // Roll the input bytes over the axes, repeated as many
    // times as needed to cover `dim` slots. Each axis is the
    // mean of its assigned bytes, normalised to `[-1, 1]`.
    for (i, slot) in v.iter_mut().enumerate() {
        let idx = i % bytes.len();
        // Mix in a position-dependent offset so that the same
        // byte at different positions lands on different axes.
        let raw = bytes[idx] as f32 / 255.0 - 0.5;
        *slot = (raw * 2.0 + (i as f32 * 0.001)).clamp(-1.0, 1.0);
    }
    v
}

impl Resource for EmbedderResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let text = match &input {
            Value::String(s) => s.clone(),
            Value::Object(_) => input
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| "embedder: missing 'text' field".to_string())?
                .to_string(),
            _ => return Err("embedder: input must be a string or {text: ...}".to_string()),
        };
        let dim = input
            .get("dim")
            .and_then(Value::as_u64)
            .map(|d| d as usize)
            .unwrap_or(self.dim)
            .clamp(1, MAX_DIM);
        let vector = project(&text, dim);
        Ok(json!({ "vector": vector }))
    }
}

/// Build a fresh `EmbedderResource` with the default dim. Boot
/// uses this; tests can use [`EmbedderResource::with_dim`].
pub fn handler() -> Arc<EmbedderResource> {
    Arc::new(EmbedderResource::new())
}

pub fn embedder_plugin() -> Arc<dyn Plugin> {
    plugin_with(
        "embedder",
        vec![Injection::from("slot:embed")],
        |ctx: Context, _cfg: ()| async move {
            let slot: Arc<Slot<EmbedderResource>> = ctx.require("slot:embed")?;
            ctx.logger().log(
                LogLevel::Info,
                format!(
                    "embedder plugin: slot={} cap_id={}",
                    slot.id().raw(),
                    slot.capability()
                        .map(|c| c.id().to_string())
                        .unwrap_or_else(|| "(empty)".to_string()),
                )
            );
            Ok(())
        },
    )
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn invoke(resource: &EmbedderResource, json_body: &str) -> Result<Value, String> {
        resource.invoke(serde_json::from_str(json_body).unwrap())
    }

    #[test]
    fn default_dim_is_eight() {
        assert_eq!(EmbedderResource::new().dim(), 8);
        assert_eq!(DEFAULT_DIM, 8);
    }

    #[test]
    fn string_input_is_accepted() {
        let r = EmbedderResource::new();
        let out = invoke(&r, "\"hello\"").unwrap();
        let v = out.get("vector").unwrap().as_array().unwrap();
        assert_eq!(v.len(), 8);
    }

    #[test]
    fn object_input_with_text_is_accepted() {
        let r = EmbedderResource::new();
        let out = invoke(&r, "{\"text\":\"hello\"}").unwrap();
        let v = out.get("vector").unwrap().as_array().unwrap();
        assert_eq!(v.len(), 8);
    }

    #[test]
    fn dim_override_is_respected() {
        let r = EmbedderResource::new();
        let out = invoke(&r, "{\"text\":\"hi\",\"dim\":4}").unwrap();
        let v = out.get("vector").unwrap().as_array().unwrap();
        assert_eq!(v.len(), 4);
    }

    #[test]
    fn dim_is_clamped_to_max() {
        let r = EmbedderResource::new();
        let out = invoke(&r, "{\"text\":\"hi\",\"dim\":9999}").unwrap();
        let v = out.get("vector").unwrap().as_array().unwrap();
        assert_eq!(v.len(), MAX_DIM);
    }

    #[test]
    fn dim_is_clamped_to_min() {
        let r = EmbedderResource::new();
        let out = invoke(&r, "{\"text\":\"hi\",\"dim\":0}").unwrap();
        let v = out.get("vector").unwrap().as_array().unwrap();
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn same_text_yields_same_vector() {
        let r = EmbedderResource::new();
        let a = invoke(&r, "\"deterministic\"").unwrap();
        let b = invoke(&r, "\"deterministic\"").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn different_text_yields_different_vector() {
        let r = EmbedderResource::new();
        let a = invoke(&r, "\"alpha\"").unwrap();
        let b = invoke(&r, "\"beta\"").unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn empty_text_yields_zero_vector() {
        let r = EmbedderResource::new();
        let out = invoke(&r, "\"\"").unwrap();
        let v = out.get("vector").unwrap().as_array().unwrap();
        for x in v {
            assert_eq!(x.as_f64().unwrap(), 0.0);
        }
    }

    #[test]
    fn missing_text_is_error() {
        let r = EmbedderResource::new();
        let err = invoke(&r, "{}").unwrap_err();
        assert!(err.contains("missing 'text'"));
    }

    #[test]
    fn non_string_non_object_input_is_error() {
        let r = EmbedderResource::new();
        let err = invoke(&r, "42").unwrap_err();
        assert!(err.contains("must be a string"));
    }

    #[test]
    fn manifest_publishes_embed_action() {
        let m = crate::plugins::embedder::manifest::manifest();
        let authority = &m.exposes[0].authority;
        assert_eq!(authority.operation_for("embed").unwrap(), "EMBED");
        assert!(!m.exposes[0].streaming, "embed must be sync, not streaming");
    }

    #[test]
    fn vectors_are_in_unit_range() {
        let r = EmbedderResource::new();
        let out = invoke(&r, "\"test payload\"").unwrap();
        let v = out.get("vector").unwrap().as_array().unwrap();
        for x in v {
            let f = x.as_f64().unwrap();
            assert!((-1.0..=1.0).contains(&f), "axis {f} out of [-1,1]");
        }
    }
}
