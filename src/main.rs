//! Odyssey entry point — boots the capability kernel + HTTP bridge.
//!
//! Loads every manifest under `src/plugins/`, mints a typed
//! `Capability<R>` for each `[[exposes]]`, registers slots, and
//! serves the HTTP bridge on 127.0.0.1:3030 until ctrl-C.
//!
//! The capability experiments are run via `cargo test
//! tests/{alpha,beta,gamma,delta,epsilon,zeta}/` — this binary
//! is the runtime.

use odyssey::runtime::lifecycle;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    lifecycle::run().await
}
