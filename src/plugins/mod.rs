//! Plugin modules — one directory per plugin, each containing:
//!
//! - `mod.rs`     — module entry, re-exports the public API
//! - `handler.rs` — the actual plugin code (resource + handler + plugin fn)
//! - `<name>.toml` — the manifest (data contract)
//! - data files   — anything the plugin needs (e.g. `*.wat` for WASM)

pub mod agent;
pub mod broker;
pub mod channel;
pub mod counter;
pub mod echo;
pub mod echo_cdylib;
pub mod echo_chain;
pub mod echo_wasm;
pub mod generator;
pub mod reverse;
pub mod sandbox;
pub mod slow;
pub mod stream_echo;