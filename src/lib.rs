//! Odyssey library — the shared module tree for both the legacy
//! `odyssey` binary (`cargo run --bin odyssey`) and the Phase 1
//! `lab` binary (`cargo run --bin lab -- <name>`).
//!
//! Splitting the modules behind a lib crate lets the labs skip the
//! cordis boot, the HTTP bridge, and the full plugin set, while the
//! main binary still exercises everything end-to-end.

pub mod capability;
pub mod host;
pub mod lab;
pub mod plugins;