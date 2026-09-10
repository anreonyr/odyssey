//! Channel + Consumer resources.
//!
//! `ChannelResource` owns the sender; `ConsumerResource` owns the
//! receiver. They are constructed together by `channel_pair(...)`,
//! which returns two `Arc`s that the host installs at distinct slots
//! under different capability names.
//!
//! The two halves share a `mpsc::channel`. The producer side is a
//! sync capability — `invoke` forwards a JSON message. The consumer
//! side is a streaming capability — `open` returns a
//! `Receiver<CapabilityChunk>` that yields each incoming message
//! and a final `Done` chunk when the producer drops. The two
//! shapes are deliberately different: sending is request/response,
//! subscribing is a stream. Both are `Resource` implementations so
//! the kernel treats them uniformly.
//!
//! Both sides are revoked uniformly — `cspace.revoke(slot)` on either
//! half severs the connection (the producer's `invoke` then errors,
//! the consumer's open stream terminates).

use std::sync::Arc;

use cordis::{plugin_with, Context, Injection, LogLevel, Plugin};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::kernel::{CapabilityChunk, Resource, Slot};

/// Producer-side resource. Holds the sender; calling `invoke`
/// sends a message. EXECUTE-gated via the holding capability.
pub struct ChannelResource {
    name: String,
    tx: mpsc::Sender<Value>,
}

impl Resource for ChannelResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let msg = input.get("message").cloned().unwrap_or(input);
        // Try to send without blocking forever — a dropped receiver
        // returns an error here, which the kernel maps to "channel
        // closed" without panicking.
        match self.tx.try_send(msg) {
            Ok(()) => Ok(json!({ "channel": self.name, "status": "sent" })),
            Err(mpsc::error::TrySendError::Full(_)) => Err(format!(
                "{}: channel full — backpressure",
                self.name
            )),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(format!(
                "{}: channel closed (consumer dropped)",
                self.name
            )),
        }
    }
}

/// Consumer-side resource. Holds the receiver; calling `open`
/// returns a stream of `CapabilityChunk` items — one per incoming
/// message, plus a terminal `Done` when the producer drops. READ-
/// gated via the holding capability.
pub struct ConsumerResource {
    name: String,
    rx: Arc<tokio::sync::Mutex<mpsc::Receiver<Value>>>,
}

impl Resource for ConsumerResource {
    /// The consumer is a streaming capability. `invoke` is intentionally
    /// not implemented — a `Resource` must override one or the other
    /// (sync or stream); the consumer has no meaningful synchronous
    /// shape.
    fn open(
        &self,
        _input: Value,
    ) -> Result<mpsc::Receiver<CapabilityChunk>, String> {
        // Wire a fresh mpsc::channel<CapabilityChunk> and spawn a
        // background task that pulls from the source receiver and
        // forwards each Value as a CapabilityChunk::Item. When the
        // source returns Disconnected (producer dropped or revoked),
        // we send Done and exit.
        //
        // The new mpsc::channel lives as long as either side holds a
        // reference. We drop the local sender at task end; callers
        // hold the receiver.
        let (out_tx, out_rx) = mpsc::channel::<CapabilityChunk>(16);
        let rx = Arc::clone(&self.rx);
        let name = self.name.clone();
        tokio::spawn(async move {
            loop {
                // Acquire the source receiver briefly, pull one
                // message, release. This is the streaming shape:
                // each pull awaits; if the source is in another pull
                // (some other `open` call), the lock waits too.
                let msg = {
                    let mut g = match rx.try_lock() {
                        Ok(g) => g,
                        Err(_) => {
                            let _ = out_tx
                                .send(CapabilityChunk::Item(json!({
                                    "channel": name,
                                    "error": "consumer busy in another call"
                                })))
                                .await;
                            return;
                        }
                    };
                    g.recv().await
                };
                match msg {
                    Some(v) => {
                        if out_tx.send(CapabilityChunk::Item(v)).await.is_err() {
                            return;
                        }
                    }
                    None => {
                        let _ = out_tx.send(CapabilityChunk::Done).await;
                        return;
                    }
                }
            }
        });
        Ok(out_rx)
    }
}

/// Build a channel pair. Returns `(channel_arc, consumer_arc)`.
pub fn channel_pair(
    name: impl Into<String>,
    buffer: usize,
) -> (Arc<ChannelResource>, Arc<ConsumerResource>) {
    let name = name.into();
    let (tx, rx) = mpsc::channel(buffer.max(1));
    let channel = Arc::new(ChannelResource {
        name: name.clone(),
        tx,
    });
    let consumer = Arc::new(ConsumerResource {
        name,
        rx: Arc::new(tokio::sync::Mutex::new(rx)),
    });
    (channel, consumer)
}

/// Plugin fiber — log only; the resource bodies carry the logic.
pub fn channel_plugin() -> Arc<dyn Plugin> {
    plugin_with(
        "channel",
        vec![
            Injection::from("slot:channel_a"),
            Injection::from("slot:consumer_a"),
        ],
        |ctx: Context, _cfg: ()| async move {
            let chan: Arc<Slot<ChannelResource>> = ctx.require("slot:channel_a")?;
            let cons: Arc<Slot<ConsumerResource>> = ctx.require("slot:consumer_a")?;
            ctx.logger().log(
                LogLevel::Info,
                format!(
                    "channel plugin: chan slot={} cons slot={}",
                    chan.id().raw(),
                    cons.id().raw()
                ),
            );
            Ok(())
        },
    )
}