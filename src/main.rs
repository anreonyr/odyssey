//! Odyssey entry point — boots the capability kernel + HTTP bridge.
//!
//! Loads every manifest under `src/plugins/`, mints a typed
//! `Capability<R>` for each `[[exposes]]`, registers slots, and
//! serves the HTTP bridge on 127.0.0.1:3030 until Ctrl-C.
//!
//! The capability experiments are run via `cargo test
//! tests/{alpha,beta,gamma,delta}/` — this binary is the runtime.

use odyssey::boot::boot;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    boot::run().await
}