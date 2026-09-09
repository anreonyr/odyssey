//! Generator plugin — mock LLM-style streaming generator.

use std::sync::Arc;
use std::time::Duration;

use cordis::{plugin_with, Context, Injection, LogLevel, Plugin};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::capability::{CapabilityChunk, CapabilityService, CapabilityToken, StreamInvoke};

const TICK: Duration = Duration::from_millis(15);

struct GeneratorHandler;

impl StreamInvoke for GeneratorHandler {
    fn stream(&self, input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String> {
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

pub fn handler() -> Arc<dyn StreamInvoke> {
    Arc::new(GeneratorHandler)
}

pub fn generator_plugin() -> Arc<dyn Plugin> {
    plugin_with(
        "generator",
        vec![
            Injection::from("cap:generate"),
            Injection::from("capability_service"),
        ],
        |ctx: Context, _cfg: ()| async move {
            let stream_token: Arc<CapabilityToken> = ctx.require("cap:generate")?;
            let cap_svc: Arc<CapabilityService> = ctx.require("capability_service")?;
            cap_svc.register(stream_token)?;

            ctx.logger().log(
                LogLevel::Info,
                "generator plugin: streaming LLM-shape capability activated".into(),
            );
            Ok(())
        },
    )
}