//! Manifest data shape + builder + TOML loader + validate.
//!
//! Single file — the Phase 5 split (types / builder / load / mod)
//! was justified while personality owned these types; in the
//! new layout the manifest is a leaf shared value type, and
//! one file is enough.

pub mod manifest;
