//! Odyssey library — capability kernel + personality orchestrator.
//!
//! Phase 8 three-layer split:
//!
//! - `core` — pure value types + abstract traits. No
//!   `Instant::now()` direct calls (use `clock::Clock`), no
//!   filesystem, no capability kernel implementation.
//!   Both `capability` and `personality` depend on `core`; `core`
//!   depends on nothing internal to the crate.
//! - `capability` — kernel implementation: `Capability<R>`,
//!   `Slot<R>`, `CapabilitySpace`, `QuotaSpec`/`QuotaState`/`CapabilityBudget`.
//!   Depends only on `core`.
//! - `personality` — orchestration: manifest resolution,
//!   lifecycle (mint / ruin / serve), HTTP bridge. Depends only
//!   on `core`.
//!
//! The library has no plugins. Workspace-member `builtins/`
//! provides the typed plugin handlers; the example binary
//! `examples/basic.rs` wires them in.

pub mod capability;
pub mod core;
pub mod personality;
