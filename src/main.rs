//! Odyssey entry point — full 7-phase boot + HTTP bridge.
//!
//! Phase 1's primary experimental surface is the `lab` binary
//! (`cargo run --bin lab -- <name>`). This binary keeps the legacy
//! demo so the HTTP bridge, the full plugin set, and the rev/
//! grant/transfer ops continue to work end-to-end.

use odyssey::host::boot;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    boot::run().await
}