//! Core contracts — abstract traits shared by `capability` and `personality`.
//!
//! Phase 8 split: these traits live in `core` so both the kernel
//! impl (`capability::*`) and the personality impl
//! (`personality::lifecycle::*`) can depend on them without a
//! cycle.
//!
//! - `resource` — the kernel-side contract a handler implements.
//! - `builtin` — minimal contract a workspace-member builtin
//!   implements (manifest only — typed mint dispatch lives on the
//!   concrete builtin struct).

pub mod builtin;
pub mod resource;

pub use builtin::BuiltinManifest;
pub use resource::Resource;
