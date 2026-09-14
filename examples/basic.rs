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

use std::sync::Arc;

use odyssey::personality::lifecycle::run::run;
use odyssey_builtins::{database, echo, reverse};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    run(
        Arc::new(echo::EchoBuiltin),
        Arc::new(reverse::ReverseBuiltin),
        Arc::new(database::DatabaseBuiltin),
    )
    .await
}
