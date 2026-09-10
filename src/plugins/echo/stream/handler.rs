//! echo_stream plugin — `EchoStreamResource: Resource`. Stream only.
//!
//! Renamed from `stream_echo` to `echo_stream` to group it
//! under the echo family more naturally (echo / echo_chain /
//! echo_stream all share the `echo_` prefix).

use std::sync::Arc;
use std::time::Duration;

use cordis::{plugin_with, Context, Injection, LogLevel, Plugin};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::capability::{CapabilityChunk, Resource, Slot};

const CHANNEL_CAPACITY: usize = 8;
const TICK: Duration = Duration::from_millis(20);

pub struct EchoStreamResource;

impl Resource for EchoStreamResource {
    fn open(&self, input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String> {
        let text = input
            .as_str()
            .ok_or_else(|| "echo_stream: input must be a string".to_string())?
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

pub fn handler() -> Arc<EchoStreamResource> {
    Arc::new(EchoStreamResource)
}

pub fn echo_stream_plugin() -> Arc<dyn Plugin> {
    plugin_with(
        "echo_stream",
        vec![Injection::from("slot:echo_stream")],
        |ctx: Context, _cfg: ()| async move {
            let slot: Arc<Slot<EchoStreamResource>> = ctx.require("slot:echo_stream")?;
            ctx.logger().log(
                LogLevel::Info,
                format!(
                    "echo_stream plugin: slot={} cap_id={}",
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
