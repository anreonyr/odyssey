//! Generator — mock LLM-style streaming resource.

use std::sync::Arc;
use std::time::Duration;

use cordis::{plugin_with, Context, Injection, LogLevel, Plugin};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::capability::{
    AnyCapability, Capability, CapabilityChunk, CapabilityService, StreamKind, StreamResource,
};

const TICK: Duration = Duration::from_millis(15);

pub struct GeneratorResource;

impl StreamResource for GeneratorResource {
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
        vec![
            Injection::from("cap:generate"),
            Injection::from("capability_service"),
        ],
        |ctx: Context, _cfg: ()| async move {
            let gen_cap: Arc<Capability<GeneratorResource, StreamKind>> = ctx.require("cap:generate")?;
            let cap_svc: Arc<CapabilityService> = ctx.require("capability_service")?;
            ctx.logger().log(
                LogLevel::Info,
                "generator plugin: streaming LLM-shape capability activated".into(),
            );
            cap_svc.register(gen_cap as Arc<dyn AnyCapability>)?;
            Ok(())
        },
    )
}