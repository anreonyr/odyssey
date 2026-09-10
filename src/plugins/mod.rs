//! Plugin modules — one directory per plugin, each containing:
//!
//! - `mod.rs`     — module entry, re-exports the public API
//! - `handler.rs` — the actual plugin code (resource + handler + plugin fn)
//! - `<name>.toml` — the manifest (data contract)
//! - data files   — anything the plugin needs (e.g. `*.wat` for WASM)
//!
//! Two groups:
//!
//! - **Runtime plugins** (declared at the top level): compiled into
//!   the host binary and minted at boot. They appear in `cargo run`
//!   output and are reachable through the HTTP bridge.
//! - **Test-only plugins** (under [`test_only`]): exercise the
//!   capability kernel in property tests but are not loaded at boot
//!   — they need specific capability wiring the host doesn't know
//!   how to do generically. The boot manifest walker skips this
//!   subtree.
//!
//! Designs for *future* loaders (WASM via wasmtime, cdylib via
//! libloading, subprocess over stdio/tcp/uds) live under
//! `docs/deferred/` rather than as empty placeholder modules here.
//! When the loader lands, the manifest moves back to
//! `src/plugins/<name>/` and gains a real `mod.rs`.

pub mod database;
pub mod echo;
pub mod embedder;
pub mod generator;
pub mod reverse;
pub mod sandbox;
pub mod slow;

/// Plugins exercised by the test crates but not loaded at boot.
/// Reachable from tests as `odyssey::plugins::test_only::counter` etc.
pub mod test_only {
    pub mod agent;
    pub mod broker;
    pub mod channel;
    pub mod counter;
}
