//! stream_echo — possession of `Slot<StreamEchoResource, StreamKind>`.

use std::sync::Arc;
use std::time::Duration;

use cordis::{plugin_with, Context, Injection, LogLevel, Plugin};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::capability::{
    CapabilityChunk, StreamKind, StreamResource,
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
        vec![Injection::from("slot:stream_echo")],
        |ctx: Context, _cfg: ()| async move {
            let slot: Arc<crate::capability::Slot<StreamEchoResource, StreamKind>> =
                ctx.require("slot:stream_echo")?;
            ctx.logger().log(
                LogLevel::Info,
                format!(
                    "stream_echo plugin: slot={} cap_id={}",
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