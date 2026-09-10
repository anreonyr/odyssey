//! β.3 — Capability-controlled communication (P4).
//!
//! A `Capability<Channel>` is the producer's authority to send;
//! revoking the channel slot severs the connection.

use odyssey::capability::Slot;
use serde_json::json;

#[test]
fn channel_send_fails_after_revoke() {
    let (space, factory) = crate::common::boot();
    let (chan_handler, _cons_handler) =
        odyssey::plugins::channel::channel_pair("p4", 16);
    let pid = odyssey::kernel::manifest::PluginId {
        name: "channel".into(),
        version: "0.1.0".into(),
    };
    let chan_decl = odyssey::kernel::manifest::CapabilityDecl {
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

    let chan = Slot::<odyssey::plugins::channel::ChannelResource>::new(space.clone(), chan_slot);
    assert!(chan.invoke(json!({"message": "hi"})).is_ok());

    assert!(space.revoke(chan_slot));

    let post = chan.invoke(json!({"message": "should-fail"}));
    assert!(post.is_err(), "expected send error after revoke");
}