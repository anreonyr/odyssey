//! Odyssey basic example — boots the capability kernel + HTTP bridge.
//!
//! Phase 8: this example binary lives at `examples/basic.rs`
//! (not in the library binary) to break the cyclic dependency
//! between the odyssey library and the `odyssey-builtins`
//! workspace member. Builtins depend on odyssey (the library);
//! the example depends on BOTH. The library itself stays
//! plugin-free.
//!
//! Wires the three concrete builtins (echo / reverse / database)
//! into the personality orchestrator and serves the HTTP bridge
//! on `127.0.0.1:3030` until Ctrl-C.
//!
//! Phase 9: the example demonstrates the typed mint + revoke +
//! derive shape that the runtime exposes. After the orchestrator
//! finishes minting, the example looks up one of the slots it
//! minted (echo), constructs a typed `Slot<EchoResource>`, holds
//! it for the lifetime of the program, and demonstrates that
//! typed-slot dispatch works against the same `Capability<R>`
//! the HTTP bridge sees. The slot reference stays valid after
//! the orchestrator returns; revoke would clear the slot
//! contents without invalidating the reference (a follow-up
//! example can add a `/revoke` HTTP route for that demo).

use std::sync::Arc;

use odyssey::capability::handle::slot::Slot;
use odyssey::capability::enforce::space::CapabilitySpace;
use odyssey::core::contract::builtin::BuiltinManifest;
use odyssey::core::identity::ids::PluginId;
use odyssey::personality::lifecycle::mint::CapabilityFactory;
use odyssey_builtins::{database, echo, reverse};

/// Phase 9 demo: drive the orchestrator's mint path with a
/// minimal in-process test of the typed-slot shape. Returns
/// the cspace + the SlotId of the echo capability so the
/// outer example can demonstrate typed dispatch.
///
/// In the production path the cspace is owned by `run()` and
/// passed into the HTTP bridge; this demo version stays
/// in-process so the typed-slot possession is visible
/// without booting the HTTP listener.
fn demo_typed_slot() -> Result<(), Box<dyn std::error::Error>> {
    let cspace = CapabilitySpace::new();
    let factory = CapabilityFactory::with_clock(cspace.clone(), Arc::new(odyssey::core::clock::clock::SystemClock));

    // Drive the orchestrator's mint path with a concrete
    // builtin — the orchestrator uses the same code shape
    // (`builtin.mint(factory, plugin, decl, kind, budget)`),
    // so this exercises every piece of the typed mint path.
    let builtin = echo::EchoBuiltin;
    let manifest = builtin.manifest();
    let decl = &manifest.exposes[0];
    let slot_id = builtin.mint(
        &factory,
        &PluginId { name: "echo".into(), version: "0.1.0".into() },
        decl,
        odyssey::core::identity::kind::CapKind::Sync,
        odyssey::capability::enforce::quota::CapabilityBudget::new(5000),
    )?;

    // Construct the typed slot from the SlotId — same path a
    // plugin uses to bind against a received capability.
    let slot: Slot<echo::EchoResource> = Slot::new(cspace.clone(), slot_id);

    let result = slot.invoke(serde_json::json!({"hello": "world"}))?;
    println!("[demo] typed-slot dispatch result: {result}");

    // The reference stays valid; revoke would clear the slot
    // contents without invalidating `slot` itself.
    println!(
        "[demo] slot id {:?} holds capability: {:?}",
        slot.id(),
        slot.meta().map(|m| m.name)
    );
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Phase 9: the typed-slot demo runs in the same process as
    // the HTTP bridge; it boots a private cspace + factory and
    // exercises typed-slot possession against an echo slot.
    demo_typed_slot()?;

    // Production path: hand the three concrete builtins to the
    // orchestrator; the orchestrator boots its own cspace +
    // factory, resolves, mints, serves HTTP until Ctrl-C, then
    // tears down in reverse mint order.
    odyssey::personality::lifecycle::run::run(
        Arc::new(echo::EchoBuiltin),
        Arc::new(reverse::ReverseBuiltin),
        Arc::new(database::DatabaseBuiltin),
    )
    .await
}
