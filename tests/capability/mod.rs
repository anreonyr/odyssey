//! κ — Revocable marker direct verification (Phase 4 P4.8 review loop).
//!
//! Round-3 reviewer finding: the Revocable marker added in round 2
//! was unverified by any direct test. This module pins the typed-cap
//! contract that round-2 P0-A fixed (marker wrapped in `Arc` so clones
//! observe `cspace::revoke`) and round-2 P0-B sealed (`Capability::derive`
//! is `pub(crate)`, so release builds can't amplify rights via the
//! typed-cap path).

mod revocable_marker;

#[path = "../common/mod.rs"]
mod common;