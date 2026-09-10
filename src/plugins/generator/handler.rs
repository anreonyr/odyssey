//! Generator plugin — `GeneratorResource: Resource`. Stream only.
//!
//! Phase 3 P3.3 — the resource carries an `Arc<dyn Model>`;
//! the boot selects the model via the `GENERATOR_MODEL` env
//! var (`mock` / `markov`, default `markov`). The handler's
//! shape is unchanged: `open()` returns an
//! `mpsc::Receiver<CapabilityChunk>` and chunks are paced at
//! `TICK` (15ms) per token so the streaming latency profile
//! matches the original mock.

use std::sync::Arc;
use std::time::Duration;

use cordis::{plugin_with, Context, Injection, LogLevel, Plugin};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::capability::{CapabilityChunk, Resource, Slot};
use crate::plugins::generator::model::Model;

const TICK: Duration = Duration::from_millis(15);

pub struct GeneratorResource {
    model: Arc<dyn Model>,
}

impl Resource for GeneratorResource {
    fn open(&self, input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String> {
        let prompt = input
            .as_str()
            .ok_or_else(|| "generate: input must be a string".to_string())?
            .to_string();
        // Phase 3 P3.3 — generate synchronously (CPU-bound but
        // bounded by MAX_TOKENS), then stream the tokens at
        // TICK pacing. This keeps the streaming API stable
        // while letting the model be anything that returns a
        // Vec<String>.
        let tokens = self.model.generate(&prompt, None)?;
        let (tx, rx) = mpsc::channel(16);
        tokio::spawn(async move {
            for tok in tokens {
                if tx.send(CapabilityChunk::Item(json!(tok))).await.is_err() {
                    return;
                }
                tokio::time::sleep(TICK).await;
            }
            let _ = tx.send(CapabilityChunk::Item(json!("[end]"))).await;
            let _ = tx.send(CapabilityChunk::Done).await;
        });
        Ok(rx)
    }
}

/// Build a `GeneratorResource` carrying `model`. Boot uses
/// [`crate::plugins::generator::model::ModelKind::build`] to
/// select the model from `GENERATOR_MODEL`.
pub fn handler(model: Arc<dyn Model>) -> Arc<GeneratorResource> {
    Arc::new(GeneratorResource { model })
}

pub fn generator_plugin() -> Arc<dyn Plugin> {
    plugin_with(
        "generator",
        vec![Injection::from("slot:generate")],
        |ctx: Context, _cfg: ()| async move {
            let slot: Arc<Slot<GeneratorResource>> = ctx.require("slot:generate")?;
            ctx.logger().log(
                LogLevel::Info,
                format!(
                    "generator plugin: slot={} cap_id={}",
                    slot.id().raw(),
                    slot.capability()
                        .map(|c| c.id().to_string())
                        .unwrap_or_else(|| "(empty)".to_string()),
                )
                .into(),
            );
            Ok(())
        },
    )
}