//! stream_echo — typed resource `StreamEchoResource: StreamResource`.

use std::sync::Arc;
use std::time::Duration;

use cordis::{plugin_with, Context, Injection, LogLevel, Plugin};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::capability::{
    AnyCapability, Capability, CapabilityChunk, CapabilityService, StreamKind, StreamResource,
};

const CHANNEL_CAPACITY: usize = 8;
const TICK: Duration = Duration::from_millis(20);

pub struct StreamEchoResource;

impl StreamResource for StreamEchoResource {
    fn open(&self, input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String> {
        let text = input
            .as_str()
            .ok_or_else(|| "stream_echo: input must be a string".to_string())?
            .to_string();

        let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
        tokio::spawn(async move {
            for ch in text.chars() {
                if tx.send(CapabilityChunk::Item(json!(ch.to_string()))).await.is_err() {
                    return;
                }
                tokio::time::sleep(TICK).await;
            }
            let _ = tx.send(CapabilityChunk::Done).await;
        });
        Ok(rx)
    }
}

pub fn handler() -> Arc<StreamEchoResource> {
    Arc::new(StreamEchoResource)
}

pub fn stream_echo_plugin() -> Arc<dyn Plugin> {
    plugin_with(
        "stream_echo",
        vec![
            Injection::from("cap:stream_echo"),
            Injection::from("capability_service"),
        ],
        |ctx: Context, _cfg: ()| async move {
            let stream_cap: Arc<Capability<StreamEchoResource, StreamKind>> =
                ctx.require("cap:stream_echo")?;
            let cap_svc: Arc<CapabilityService> = ctx.require("capability_service")?;
            ctx.logger().log(
                LogLevel::Info,
                "stream_echo plugin: streaming capability activated".into(),
            );
            cap_svc.register(stream_cap as Arc<dyn AnyCapability>)?;
            Ok(())
        },
    )
}