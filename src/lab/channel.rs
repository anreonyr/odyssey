//! Lab — Channel (Phase 2 P4).
//!
//! Demonstrates capability-controlled communication: Producer and
//! Consumer each hold a typed Capability; messages flow through a
//! tokio mpsc; revocation of either side severs the connection.
//!
//! ```text
//! Producer --invoke--> Capability<Channel>  --mpsc--> Capability<Consumer> --invoke--> reply
//! ```

use crate::capability::{CapabilityRights, OperationRights};
use crate::host::factory::CapabilityFactory;
use crate::plugins::channel::{channel_pair, ChannelResource, ConsumerResource};
use crate::capability::Slot;
use serde_json::json;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n== Lab :: Channel ==\n");

    let cspace = crate::capability::CapabilitySpace::new();
    let factory = CapabilityFactory::new(cspace.clone());

    // Build the channel pair: producer side + consumer side.
    let (chan_handler, cons_handler) = channel_pair("phase2.demo", 16);

    let pid = crate::host::manifest::PluginId {
        name: "channel".into(),
        version: "0.1.0".into(),
    };

    let chan_decl = crate::host::manifest::CapabilityDecl {
        name: "channel_a".into(),
        in_type: "object".into(),
        out_type: "object".into(),
        streaming: false,
    };
    let chan_slot = factory.mint::<ChannelResource>(
        crate::capability::CapKind::Sync,
        &chan_decl,
        &pid,
        crate::capability::CapabilityBudget::new(1000),
        chan_handler,
    );

    let cons_decl = crate::host::manifest::CapabilityDecl {
        name: "consumer_a".into(),
        in_type: "null".into(),
        out_type: "object".into(),
        streaming: false,
    };
    let cons_slot = factory.mint::<ConsumerResource>(
        crate::capability::CapKind::Sync,
        &cons_decl,
        &pid,
        crate::capability::CapabilityBudget::new(1000),
        cons_handler,
    );

    let chan = Slot::<ChannelResource>::new(cspace.clone(), chan_slot);
    let cons = Slot::<ConsumerResource>::new(cspace.clone(), cons_slot);

    println!("  channel slot={chan_slot} (producer)");
    println!("  consumer slot={cons_slot}");

    println!("\n  --- producer sends ---");
    for n in 0..3 {
        match chan.invoke(json!({ "message": format!("hello-{n}") })) {
            Ok(v) => println!("  ✓ sent: {v}"),
            Err(e) => println!("  ✗ send: {e}"),
        }
    }

    println!("\n  --- consumer reads ---");
    for _ in 0..3 {
        match cons.invoke(json!({})) {
            Ok(v) => println!("  ✓ recv: {v}"),
            Err(e) => println!("  ✗ recv: {e}"),
        }
    }

    println!("\n  --- restrict consumer to read-only (drops any op bits beyond READ) ---");
    let readonly_id = cspace.restrict::<ConsumerResource>(
        cons_slot,
        CapabilityRights {
            operations: OperationRights::READ,
            timeout_ms: 1000,
        },
        "consumer_ro".into(),
    )?;
    println!("  ✓ restricted consumer slot={readonly_id}");

    println!("\n  --- revoke channel slot ---");
    let removed = cspace.revoke(chan_slot);
    println!("  revoke(channel) → {removed}");
    let post_chan = Slot::<ChannelResource>::new(cspace.clone(), chan_slot);
    match post_chan.invoke(json!({ "message": "should-fail" })) {
        Ok(_) => println!("  ✗ channel still works after revoke"),
        Err(e) => println!("  ✓ channel denied: {e}"),
    }

    println!("\n  done.");
    Ok(())
}