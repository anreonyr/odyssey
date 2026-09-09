//! Odyssey entry point. All the boot orchestration lives in
//! `host::boot::run`; this file just wires up the modules and calls it.

mod capability;
mod host;
mod plugins;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    host::boot::run().await
}