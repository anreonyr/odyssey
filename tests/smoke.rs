//! Smoke test — builtins successfully round-trip through the
//! orchestrator's typed mint API.
//!
//! Phase 9.5: replaced the previous `examples/basic.rs::demo_
//! typed_slot` helper, which lived next to the production
//! boot path and could silently drift from it. This test
//! is a CI gate: any change to the orchestrator's mint
//! surface (factory signature, `Slot<R>` construction,
//! builtin's `Mint` trait impl) must keep this round-trip
//! working, or the test fails at compile / runtime.
//!
//! The check covers every integration point of the typed
//! mint path:
//!
//! - `CapabilitySpace::new()` — cspace construction
//! - `CapabilityFactory::with_clock(cspace, clock)` —
//!   factory setup
//! - `BuiltinManifest::manifest()` — manifest retrieval
//! - `Mint::mint(factory, plugin, decl, kind, budget)` —
//!   the trait surface Phase 9 consolidated from the three
//!   per-plugin marker traits
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

use std::sync::Arc;

use odyssey::capability::enforce::quota::CapabilityBudget;
use odyssey::capability::enforce::space::CapabilitySpace;
use odyssey::capability::handle::slot::Slot;
use odyssey::core::clock::clock::SystemClock;
use odyssey::core::contract::builtin::BuiltinManifest;
use odyssey::core::identity::ids::PluginId;
use odyssey::core::identity::kind::CapKind;
use odyssey::personality::lifecycle::mint::CapabilityFactory;
use odyssey::personality::lifecycle::run::Mint;
use odyssey_builtins::echo::{EchoBuiltin, EchoResource};

#[test]
fn echo_builtin_round_trips_through_typed_mint() {
    let cspace = CapabilitySpace::new();
    let factory = CapabilityFactory::with_clock(cspace.clone(), Arc::new(SystemClock));

    let builtin = EchoBuiltin;
    let manifest = builtin.manifest();
    let decl = &manifest.exposes[0];

    // Dispatch `mint` through the `Mint` trait object so this
    // test exercises the trait surface Phase 9 consolidated
    // (the three per-plugin marker traits folded into one
    // `Mint` trait in commit `514e5c9`). Calling `builtin.
    // mint(...)` directly would resolve to the inherent
    // method on `EchoBuiltin` and the trait import would be
    // unused — same call shape, but it would silently skip
    // the trait dispatch the orchestrator actually relies on.
    let mint_ref: &dyn Mint = &builtin;
    let slot_id = mint_ref
        .mint(
            &factory,
            &PluginId {
                name: "echo".into(),
                version: "0.1.0".into(),
            },
            decl,
            CapKind::Sync,
            CapabilityBudget::new(5000),
        )
        .expect("mint should succeed");

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
