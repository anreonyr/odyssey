//! Odyssey basic example — boots the capability kernel + HTTP bridge.
//!
//! Phase 8: this example binary lives at `examples/basic.rs`
//! (not in the library binary) to break the cyclic dependency
//! between the odyssey library and the `odyssey-builtins`
//! workspace member. Builtins depend on odyssey (the library);
//! the example depends on BOTH. The library itself stays
//! plugin-free.
//!
//! Phase 10/11: the registry of `(manifest, mint_fn, ruin_fn)`
//! triples is built here at boot from the colocated
//! `XBuiltin::register()` helpers in `builtins/src/`. Adding
//! a new builtin means adding one `XBuiltin::register()`
//! entry here and one module to `builtins/src/`; the
//! orchestrator's `run` doesn't change. Every `RuinFn` is
//! the default (just `cspace.revoke_tree` per slot) — no
//! builtin ships a custom teardown yet.
//!
//! The two agent entries come last on purpose. The agent's
//! manifests declare `requires` for the four capability
//! builtins, so the resolver emits a binding row for it and
//! mints it after its providers; listing it last keeps the
//! example's order match the mint order it produces.

use odyssey::personality::lifecycle::run::run;
use odyssey_builtins::{agent, database, echo, reverse, streaming_echo};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let plugins = vec![
        echo::EchoBuiltin::register(),
        reverse::ReverseBuiltin::register(),
        database::DatabaseBuiltin::register(),
        streaming_echo::StreamingEchoBuiltin::register(),
        agent::AgentListBuiltin::register(),
        agent::AgentDescribeBuiltin::register(),
    ];
    run(&plugins).await
}
