//! Manifest data shape + builder.
//!
//! Single file — the Phase 5 split (types / builder / load / mod)
//! was justified while personality owned these types; in the
//! new layout the manifest is a leaf shared value type, and
//! one file is enough.

// See the note in `core/clock/mod.rs`: the strict-hierarchy
// convention triggers clippy's `module_inception` warning; we
// suppress it deliberately here too.
#[allow(clippy::module_inception)]
pub mod manifest;
