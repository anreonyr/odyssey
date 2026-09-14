//! Phase 8: re-export shim.
//!
//! `meta_from_decl` and `namespace_for` now live in
//! `crate::personality::lifecycle::mint`. The factory that
//! previously lived in `host/factory.rs` is in the same place
//! after the merge — both are re-exported there.

pub use crate::personality::lifecycle::mint::{meta_from_decl, namespace_for};
