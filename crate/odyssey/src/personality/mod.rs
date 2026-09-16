//! Personality — orchestration.
//!
//! Phase 8 split: personality holds the planner-then-executor split.
//!
//! - `composition` — pure computation step (resolve manifest
//!   graph into mint order + binding table).
//! - `lifecycle` — action step (mint, ruin, serve_http).

pub mod composition;
pub mod lifecycle;
