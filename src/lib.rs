//! Odyssey library — the capability kernel + plugins.
//!
//! Three layers (Phase 5):
//!
//! - `kernel` — the pure kernel: capability data types, slot
//!   algebra, quota accounting, namespace tree, graph events.
//!   No I/O, no `Instant::now()` direct calls (use `clock::Clock`),
//!   no filesystem, no cordis.
//! - `host` — composition layer: parses manifests, resolves
//!   dependencies, mints typed caps into the cspace. Depends on
//!   `kernel`.
//! - `runtime` — adapter layer: lifecycle, HTTP bridge, plugin
//!   mint dispatch. Depends on `host` and `kernel`.
//! - `plugins` — plugin bodies. Depend on `kernel` (for the
//!   `Capability<R>` / `Resource` types they implement) and
//!   `host` (for `PluginManifest` / `CapabilityFactory`).

pub mod host;
pub mod kernel;
pub mod plugins;
pub mod runtime;
