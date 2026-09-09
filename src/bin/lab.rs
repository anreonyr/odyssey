//! `cargo run --bin lab -- <name>` — Phase 1 + Phase 2 experiments.
//!
//! ```text
//! cargo run --bin lab -- authority     # Phase 1
//! cargo run --bin lab -- delegation    # Phase 1
//! cargo run --bin lab -- revocation    # Phase 1
//! cargo run --bin lab -- composition   # Phase 1
//! cargo run --bin lab -- quota         # Phase 2 P5 (quota)
//! cargo run --bin lab -- namespace     # Phase 2 P6 (namespace tree)
//! cargo run --bin lab -- graph         # Phase 2 P6 (capability graph)
//! cargo run --bin lab -- channel       # Phase 2 P4 (capability-controlled comms)
//! cargo run --bin lab -- agent         # Phase 2 P5 (same program, different caps)
//! cargo run --bin lab -- multi_hop     # Phase 2 P2 (A → B → C delegation chain)
//! ```

use odyssey::lab;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arg = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "help".to_string());
    match arg.as_str() {
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
            println!(
                "usage: cargo run --bin lab -- <authority|delegation|revocation|composition|quota|namespace|graph|channel|agent|multi_hop>"
            );
            std::process::exit(2);
        }
    }
}