//! Authority attenuation — `Rights` (role-typed 3-bit bitflag:
//! `INVOKE | ASSIGN | REVOKE`) + `CapabilityRights`
//! (operations + timeout).

// See `core/clock/mod.rs` for the rationale.
#[allow(clippy::module_inception)]
pub mod rights;
