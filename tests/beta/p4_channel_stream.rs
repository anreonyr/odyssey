//! β.3b — ConsumerResource streams via `open` (Phase 2 P4 streaming shape).
//!
//! The consumer side of the channel is no longer a sync
//! `try_recv`-based `invoke`. It's a streaming resource: `open`
//! returns a `Receiver<CapabilityChunk>` that yields one item per
//! incoming message and a terminal `Done` when the producer drops
//! (or is revoked).

use odyssey::capability::{CapabilityChunk, Slot};
use serde_json::json;

#[tokio::test]
async fn consumer_stream_yields_messages_then_done() {
    let (space, factory) = crate::common::boot();
    let (chan_handler, cons_handler) =
        odyssey::plugins::channel::channel_pair("p4_stream", 16);
    let pid = odyssey::host::manifest::PluginId {
        name: "channel".into(),
        version: "0.1.0".into(),
    };

    // Producer (sync).
    let chan_decl = odyssey::host::manifest::CapabilityDecl {
        name: "channel_a".into(),
        in_type: "object".into(),
        out_type: "object".into(),
        streaming: false,
        ..Default::default()
    };
    let chan_slot = factory.mint::<odyssey::plugins::channel::ChannelResource>(
        odyssey::capability::CapKind::Sync,
        &chan_decl,
        &pid,
        odyssey::capability::CapabilityBudget::new(crate::common::DEFAULT_TIMEOUT_MS),
        chan_handler,
    );

    // Consumer (stream). Minted as CapKind::Stream to match the
    // manifest's `streaming = true`.
    let cons_decl = odyssey::host::manifest::CapabilityDecl {
        name: "consumer_a".into(),
        in_type: "null".into(),
        out_type: "object".into(),
        streaming: true,
        ..Default::default()
    };
    let cons_slot = factory.mint::<odyssey::plugins::channel::ConsumerResource>(
        odyssey::capability::CapKind::Stream,
        &cons_decl,
        &pid,
        odyssey::capability::CapabilityBudget::new(crate::common::DEFAULT_TIMEOUT_MS),
        cons_handler,
    );

    let chan = Slot::<odyssey::plugins::channel::ChannelResource>::new(space.clone(), chan_slot);
    let cons = Slot::<odyssey::plugins::channel::ConsumerResource>::new(space.clone(), cons_slot);

    // Send two messages via the producer before opening the stream.
    chan.invoke(json!({"message": "first"})).unwrap();
    chan.invoke(json!({"message": "second"})).unwrap();

    // Open the consumer stream.
    let mut rx = cons.open(json!({})).expect("consumer open");

    // Receive the two items (with timeout so a deadlock fails fast
    // instead of hanging the test).
    let item1 = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        rx.recv(),
    )
    .await
    .expect("first item within 2s")
    .expect("first item present");
    let item2 = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        rx.recv(),
    )
    .await
    .expect("second item within 2s")
    .expect("second item present");
    assert!(matches!(item1, CapabilityChunk::Item(_)));
    assert!(matches!(item2, CapabilityChunk::Item(_)));
    if let CapabilityChunk::Item(v) = item1 {
        assert_eq!(v, json!("first"));
    }
    if let CapabilityChunk::Item(v) = item2 {
        assert_eq!(v, json!("second"));
    }

    // Drop the producer. The ChannelResource (and its mpsc::Sender)
    // lives inside the cspace at chan_slot — dropping the typed
    // Slot handle doesn't sever the connection. cspace.revoke
    // removes the slot and the Arc count on the cap drops, which
    // drops the sender and the consumer's recv() returns None.
    space.revoke(chan_slot);
    drop(chan);
    let done = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        rx.recv(),
    )
    .await
    .expect("terminal chunk within 2s")
    .expect("terminal chunk present");
    assert!(matches!(done, CapabilityChunk::Done));
}