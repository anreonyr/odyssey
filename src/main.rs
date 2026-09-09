//! Odyssey entry point — single binary, 7-phase boot + HTTP bridge.
//!
//! ```text
//! cargo run -- boot    # default; load manifests, mint caps, start HTTP server
//! cargo run -- odyssey # alias for boot
//! ```
//!
//! Without a subcommand the binary prints usage. The 10 Phase 1+2
//! capability experiments that used to live at `cargo run -- lab <name>`
//! are now exercised as `#[test]` functions under `tests/{alpha,beta,
//! gamma,delta}/`; run them with `cargo test` (or `cargo test --
//! --nocapture` for printed transcripts).

use odyssey::host::boot;

const USAGE: &str = "usage:
  cargo run -- boot
  cargo run -- odyssey";

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    match std::env::args().nth(1).as_deref() {
        Some("boot") | Some("odyssey") | None => boot::run().await,
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    }
}