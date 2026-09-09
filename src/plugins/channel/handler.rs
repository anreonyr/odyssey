//! Channel + Consumer resources.
//!
//! `ChannelResource` owns the sender; `ConsumerResource` owns the
//! receiver. They are constructed together by `channel_pair(...)`,
//! which returns two `Arc`s that the host installs at distinct slots
//! under different capability names.
//!
//! The two halves share a `mpsc::channel`. The sender is wrapped in
//! `ChannelResource` and the receiver in `ConsumerResource`. This
//! keeps the kernel-level contract uniform — every interaction with
//! either side is just a `Resource::invoke` on a typed capability.

use std::sync::Arc;

use cordis::{plugin_with, Context, Injection, LogLevel, Plugin};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::capability::{CapabilityChunk, Resource, Slot};

/// Producer-side resource. Holds the sender; calling `invoke` sends a
/// message. EXECUTE-gated via the holding capability.
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

/// Consumer-side resource. Holds the receiver; calling `invoke` reads
/// the next message. READ-gated via the holding capability.
pub struct ConsumerResource {
    name: String,
    rx: Arc<tokio::sync::Mutex<mpsc::Receiver<Value>>>,
}

impl Resource for ConsumerResource {
    fn invoke(&self, _input: Value) -> Result<Value, String> {
        // Synchronous Resource::invoke can't await. Use blocking
        // try_recv — for the Phase 2 lab a single message is enough.
        // Phase 3 will replace this with a streaming variant.
        let mut g = self.rx.try_lock().map_err(|_| {
            format!("{}: consumer is busy in another call", self.name)
        })?;
        match g.try_recv() {
            Ok(v) => Ok(v),
            Err(mpsc::error::TryRecvError::Empty) => Err(format!(
                "{}: no message available",
                self.name
            )),
            Err(mpsc::error::TryRecvError::Disconnected) => Err(format!(
                "{}: producer dropped",
                self.name
            )),
        }
    }

    fn open(&self, _input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String> {
        // Phase 2 surfaces the underlying tokio receiver as a stream
        // so callers can subscribe to a channel with `Capability::open`.
        // We hand out the receiver wrapped — the inner channel lives
        // until either side drops.
        Err(format!(
            "{}: streaming consumer not wired in Phase 2; use invoke",
            self.name
        ))
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
                )
                .into(),
            );
            Ok(())
        },
    )
}