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
use std::time::Duration;

use odyssey::capability::enforce::quota::CapabilityBudget;
use odyssey::capability::enforce::space::CapabilitySpace;
use odyssey::capability::handle::slot::Slot;
use odyssey::core::clock::clock::SystemClock;
use odyssey::core::contract::builtin::BuiltinManifest;
use odyssey::core::identity::ids::PluginId;
use odyssey::core::identity::kind::CapKind;
use odyssey::core::meta::chunk::CapabilityChunk;
use odyssey::personality::lifecycle::mint::CapabilityFactory;
use odyssey_builtins::echo::{EchoBuiltin, EchoResource};
use odyssey_builtins::streaming_echo::{StreamingEchoBuiltin, StreamingEchoResource};
use tokio_stream::StreamExt;

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

/// Phase 11 streaming smoke test. Exercises the full
/// `Resource::open` path: mint a streaming cap, call
/// `Slot::open(...)` to get a `Receiver<CapabilityChunk>`,
/// collect the chunks, assert shape + count. Bounded by
/// `tokio::time::timeout` so a hung producer fails the test
/// rather than hanging CI.
#[tokio::test(flavor = "current_thread")]
async fn streaming_echo_builtin_round_trips_through_typed_open() {
    let cspace = CapabilitySpace::new();
    let factory = CapabilityFactory::with_clock(cspace.clone(), Arc::new(SystemClock));

    let builtin = StreamingEchoBuiltin;
    let manifest = builtin.manifest();
    let decl = &manifest.exposes[0];

    let slot_id = builtin.mint(
        &factory,
        &PluginId {
            name: "streaming_echo".into(),
            version: "0.1.0".into(),
        },
        decl,
        CapKind::Stream,
        CapabilityBudget::new(5000),
    );

    let slot: Slot<StreamingEchoResource> = Slot::new(cspace, slot_id);
    let rx = slot
        .open(serde_json::json!({"text": "hi", "count": 3}))
        .expect("open should succeed");

    // Collect all chunks within a 2-second budget. A hung
    // producer fails the test instead of hanging the runner.
    let chunks: Vec<CapabilityChunk> = tokio::time::timeout(
        Duration::from_secs(2),
        tokio_stream::wrappers::ReceiverStream::new(rx)
            .collect::<Vec<_>>(),
    )
    .await
    .expect("streaming_echo producer hung within 2s budget");

    assert_eq!(
        chunks.len(),
        4,
        "expected 3 items + 1 done = 4 chunks, got {}",
        chunks.len()
    );

    // Last chunk must be Done.
    assert!(
        matches!(chunks.last(), Some(CapabilityChunk::Done)),
        "last chunk should be Done, got {:?}",
        chunks.last()
    );

    // The 3 item chunks carry the right text and indices in
    // order.
    for (i, chunk) in chunks.iter().take(3).enumerate() {
        let CapabilityChunk::Item(value) = chunk else {
            panic!("expected Item at index {i}, got {chunk:?}");
        };
        assert_eq!(
            value.get("text").and_then(|v| v.as_str()),
            Some("hi"),
            "chunk {i} text mismatch"
        );
        assert_eq!(
            value.get("index").and_then(|v| v.as_u64()),
            Some(i as u64),
            "chunk {i} index mismatch"
        );
    }
}
