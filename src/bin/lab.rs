//! `cargo run --bin lab -- <name>` — Phase 1 capability experiments.
//!
//! ```text
//! cargo run --bin lab -- authority
//! cargo run --bin lab -- delegation
//! cargo run --bin lab -- revocation
//! cargo run --bin lab -- composition
//! ```
//!
//! Each lab boots a fresh `CapabilitySpace` + `CapabilityFactory` and
//! prints a self-explanatory transcript. Each lab answers exactly one
//! question about whether the capability model is real:
//!
//! | lab          | question                                              |
//! |--------------|-------------------------------------------------------|
//! | authority    | is a capability really authority?                     |
//! | delegation   | does authority propagate through a plugin?           |
//! | revocation   | can authority be revoked through the slot?            |
//! | composition  | does revocation propagate through composed stages?    |
//!
//! Two more properties — restrict-attenuation and budget — are
//! first-class assertions inside the four labs and are also covered
//! by the property tests in `tests/capability_lab.rs`.

use odyssey::lab;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arg = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "help".to_string());
    match arg.as_str() {
        "authority"   => lab::authority::run(),
        "delegation"  => lab::delegation::run(),
        "revocation"  => lab::revocation::run(),
        "composition" => lab::composition::run(),
        _ => {
            println!(
                "usage: cargo run --bin lab -- <authority|delegation|revocation|composition>"
            );
            std::process::exit(2);
        }
    }
}