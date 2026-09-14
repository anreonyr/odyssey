//! Phase 8: re-export shim.
//!
//! The runtime layer's contents have all moved:
//! - `mint` → `personality::lifecycle::mint`
//! - `lifecycle` → `personality::lifecycle` (with `run` etc.)
//! - `activate` (deleted; cordis activator moved into builtins)
//! - `teardown` → `personality::lifecycle::ruin`
//! - `http_bridge` → `personality::lifecycle::serve`
//!
//! This shim re-exports the legacy `runtime::*` paths so
//! out-of-tree code (the `src/main.rs` binary, the soon-to-be-
//! deleted `tests/`) continues to compile during the migration
//! window. The `runtime/` directory itself will be removed in
//! a later cleanup commit.

pub mod lifecycle;
pub mod mint;

pub use crate::personality::lifecycle::{
    mint as mint_dispatch, ruin as teardown, serve as http_bridge,
};
