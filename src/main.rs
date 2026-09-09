//! Odyssey entry point — single binary, subcommand dispatch.
//!
//! ```text
//! cargo run -- boot               # legacy 7-phase boot + HTTP bridge
//! cargo run -- lab authority      # Phase 1+2 capability experiments
//! cargo run -- lab multi_hop      # A → B → C delegation chain
//! ```
//!
//! `cargo run` with no args prints usage. The lab commands are the
//! primary Phase 1+2 surface; `boot` is preserved for the legacy
//! 7-phase demo and the HTTP bridge.

use odyssey::{host::boot, lab};

const USAGE: &str = "usage:
  cargo run -- boot
  cargo run -- lab <authority|delegation|revocation|composition|quota|namespace|graph|channel|agent|multi_hop>";

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cmd = std::env::args().nth(1);
    match cmd.as_deref() {
        Some("boot") | Some("odyssey") => boot::run().await,
        Some("lab") => run_lab(std::env::args().nth(2)),
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    }
}

fn run_lab(name: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let name = name.unwrap_or_default();
    match name.as_str() {
        // Phase 1
        "authority"   => lab::authority::run(),
        "delegation"  => lab::delegation::run(),
        "revocation"  => lab::revocation::run(),
        "composition" => lab::composition::run(),
        // Phase 2
        "quota"       => lab::quota::run(),
        "namespace"   => lab::namespace::run(),
        "graph"       => lab::graph::run(),
        "channel"     => lab::channel::run(),
        "agent"       => lab::agent::run(),
        "multi_hop"   => lab::multi_hop::run(),
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    }
}