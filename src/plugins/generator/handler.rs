//! Generator plugin — `GeneratorResource: Resource`. Stream only.

use std::sync::Arc;
use std::time::Duration;

use cordis::{plugin_with, Context, Injection, LogLevel, Plugin};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::capability::{CapabilityChunk, Resource, Slot};

const TICK: Duration = Duration::from_millis(15);

pub struct GeneratorResource;

impl Resource for GeneratorResource {
    fn open(&self, input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String> {
        let prompt = input
            .as_str()
            .ok_or_else(|| "generate: input must be a string".to_string())?
            .to_string();

        let response = if prompt.to_lowercase().contains("hello") {
            "Hello! I'm a mock generator — the framework supports any provider.".to_string()
        } else {
            format!("Mock generator received: \"{prompt}\"")
        };

        let (tx, rx) = mpsc::channel(16);
        tokio::spawn(async move {
            for ch in response.chars() {
                if tx.send(CapabilityChunk::Item(json!(ch.to_string()))).await.is_err() {
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

pub fn handler() -> Arc<GeneratorResource> {
    Arc::new(GeneratorResource)
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
                ),
            );
            Ok(())
        },
    )
}