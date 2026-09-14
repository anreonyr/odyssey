//! Personality / lifecycle — actions on the capability kernel.
//!
//! Phase 8 split: personality holds the planner-then-executor split.
//!
//! - `composition` — pure computation step (resolve manifest
//!   graph into mint order + binding table).
//! - `lifecycle` — action step (mint, ruin, serve_http).
//!
//! `ruin` and `serve` arrive in later commits; only `mint` is
//! populated at this point.

pub mod mint;
