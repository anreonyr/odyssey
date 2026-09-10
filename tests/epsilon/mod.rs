//! ε — Phase 3 capability-injection integration tests.
//!
//! These tests sit alongside the α/β/γ/δ suites but cover the
//! Phase 3 layer where capability-keyed dependency resolution
//! replaces plugin-version-keyed lookup. Today (P3.1) the suite
//! covers:
//!
//! - **ε.1–ε.7** ([`p3_1_injection`]): the resolver end-to-end
//!   — contract-keyed binding, version stability, ambiguous /
//!   unprovided / cyclic error paths, diamond topology, and the
//!   `CapabilityMeta.contract_name` carry-through.
//! - **ε.8** (in [`p3_1_injection`]): every runtime manifest
//!   declares a non-empty `contract_name` so the resolver can
//!   actually wire the runtime up.

mod p3_1_injection;

#[path = "../common/mod.rs"]
mod common;
