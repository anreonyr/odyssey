//! Smoke test — builtins successfully round-trip through the
//! orchestrator's typed mint API.
//!
//! Phase 9.5: replaced the previous `examples/basic.rs::demo_
//! typed_slot` helper, which lived next to the production
//! boot path and could silently drift from it. This test
//! is a CI gate: any change to the orchestrator's mint
//! surface (factory signature, `Slot<R>` construction,
//! builtin's `MintFn` function-pointer shape) must keep
//! this round-trip working, or the test fails at compile /
//! runtime.
//!
//! The check covers every integration point of the typed
//! mint path:
//!
//! - `CapabilitySpace::new()` — cspace construction
//! - `CapabilityFactory::with_clock(cspace, clock)` —
//!   factory setup
//! - `BuiltinManifest::manifest()` — manifest retrieval
//! - `EchoBuiltin::mint(factory, plugin, decl, kind, budget)`
//!   — the inherent typed mint method (Phase 10: the `Mint`
//!   trait surface is gone; builtins call `factory.mint(...)`
//!   directly via a non-capturing closure registered in
//!   the orchestrator's `(PluginManifest, MintFn)` registry)
//! - `Slot::new(cspace, slot_id)` — typed-slot construction
//! - `Slot::invoke(input)` — sync dispatch returning
//!   `Result<Value, CapabilityError>`
//!
//! If any of these change shape (return type, signature,
//! behaviour), this test fails. If a future builtin
//! simplifies its import (e.g. `use …::mint;` instead of
//! `use …::mint::CapabilityFactory;`), the test stays green
//! as long as the API still resolves — it asserts the
//! *behavioural* contract, not the import *shape*.
//!
//! Only `EchoBuiltin` is smoke-tested. The orchestrator
//! dispatches all three builtins (`echo` / `reverse` /
//! `database`) through the same `MintFn` shape (each
//! builtin's `register()` returns a non-capturing closure
//! of the same signature), so an echo round-trip exercises
//! the registry path; a per-builtin mint path is a thin
//! wrapper around `factory.mint(...)` and would fail
//! `cargo build` if it drifted.

use std::sync::Arc;

use odyssey::capability::enforce::quota::CapabilityBudget;
use odyssey::capability::enforce::space::CapabilitySpace;
use odyssey::capability::handle::slot::Slot;
use odyssey::core::clock::clock::SystemClock;
use odyssey::core::contract::builtin::BuiltinManifest;
use odyssey::core::identity::ids::PluginId;
use odyssey::core::identity::kind::CapKind;
use odyssey::personality::lifecycle::mint::CapabilityFactory;
use odyssey_builtins::echo::{EchoBuiltin, EchoResource};

#[test]
fn echo_builtin_round_trips_through_typed_mint() {
    let cspace = CapabilitySpace::new();
    let factory = CapabilityFactory::with_clock(cspace.clone(), Arc::new(SystemClock));

    let builtin = EchoBuiltin;
    let manifest = builtin.manifest();
    let decl = &manifest.exposes[0];

    // Phase 10: call the inherent `mint` method directly.
    // The trait object dispatch (`&dyn Mint`) was deleted
    // along with the `Mint` trait; the registry holds a
    // non-capturing closure of the same shape, so calling
    // the inherent method exercises the exact code path
    // the orchestrator's `MintFn` does.
    let slot_id = builtin.mint(
        &factory,
        &PluginId {
            name: "echo".into(),
            version: "0.1.0".into(),
        },
        decl,
        CapKind::Sync,
        CapabilityBudget::new(5000),
    );

    let slot: Slot<EchoResource> = Slot::new(cspace, slot_id);
    let input = serde_json::json!({"hello": "world"});
    let output = slot
        .invoke(input.clone())
        .expect("invoke should succeed");

    assert_eq!(
        output, input,
        "echo builtin must return its input unchanged"
    );
}
