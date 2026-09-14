//! Odyssey library — the capability kernel + plugins.
//!
//! Phase 8 three-layer split:
//!
//! - `core` — pure value types + abstract traits. No
//!   `Instant::now()` direct calls (use `clock::Clock`), no
//!   filesystem, no cordis, no capability kernel implementation.
//!   Both `capability` and `personality` depend on `core`.
//! - `capability` — kernel implementation: `Capability<R>`,
//!   `Slot<R>`, `CapabilitySpace`, `QuotaSpec`/`QuotaState`/`CapabilityBudget`.
//!   Depends only on `core`.
//! - `personality` — orchestration: manifest resolution,
//!   lifecycle (mint / ruin / serve), HTTP bridge. Depends only
//!   on `core`.
//!
//! Migration in progress: `kernel`, `host`, `runtime`, `plugins`
//! still exist alongside the new tree. The `kernel` module is
//! a thin re-export shim pointing at `crate::core::*` for the
//! leaf value-type modules that have already moved (ids, kind,
//! clock, chunk, rights, meta). Subsequent commits will retire
//! the old modules.

pub mod capability;
pub mod core;
pub mod host;
pub mod kernel;
pub mod personality;
pub mod runtime;
