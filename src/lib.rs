//! Odyssey library — the capability kernel + plugins.
//!
//! Three modules:
//!
//! - `capability` — the kernel model (CSpace, slot, capability,
//!   resource trait, rights, contract metadata).
//! - `kernel` — minting and composition: factory that turns a
//!   `PluginManifest` into a typed `Capability<R>`; pipeline that
//!   composes sync caps into stages; registry for duplicate-name
//!   detection.
//! - `boot` — runtime lifecycle: load manifests, mint typed
//!   tokens, start cordis fibers, serve the HTTP bridge, wait for
//!   Ctrl-C.
//! - `plugins` — the plugin bodies themselves (`handler.rs` +
//!   `<name>.toml` manifest per plugin).

pub mod boot;
pub mod capability;
pub mod kernel;
pub mod plugins;