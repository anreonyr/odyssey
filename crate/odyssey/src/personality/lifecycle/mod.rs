//! Personality / lifecycle — actions on the capability kernel.
//!
//! - `boot` — manifest loader + print (boot diagnostics).
//! - `lifecycle_event` — personality-side observability events
//!   (PluginActivated, ShutdownStarted, etc.).
//! - `mint` — capability factory + generic mint loop.
//! - `ruin` — reverse-mint revocation.
//! - `run` — top-level orchestrator (`run()` entry point).
//! - `serve` — HTTP bridge for capability dispatch.

pub mod boot;
pub mod lifecycle_event;
pub mod mint;
pub mod ruin;
pub mod run;
pub mod serve;
