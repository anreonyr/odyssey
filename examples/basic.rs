//! Odyssey basic example — boots the capability kernel + HTTP bridge.
//!
//! Phase 8: this example binary lives at `examples/basic.rs`
//! (not in the library binary) to break the cyclic dependency
//! between the odyssey library and the `odyssey-builtins`
//! workspace member. Builtins depend on odyssey (the library);
//! the example depends on BOTH. The library itself stays
//! plugin-free.
//!
//! Phase 10: the registry of `(manifest, mint_fn)` pairs is
//! built here at boot from the colocated `XBuiltin::register()`
//! helpers in `builtins/src/`. Adding a new builtin means
//! adding one `XBuiltin::register()` entry here and one
//! module to `builtins/src/`; the orchestrator's `run`
//! doesn't change.

use odyssey::personality::lifecycle::run::run;
use odyssey_builtins::{database, echo, reverse};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let plugins = vec![
        echo::EchoBuiltin::register(),
        reverse::ReverseBuiltin::register(),
        database::DatabaseBuiltin::register(),
    ];
    run(&plugins).await
}
