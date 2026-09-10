//! Runtime lifecycle — load manifests, mint typed tokens, start
//! cordis fibers, serve the HTTP bridge, wait for Ctrl-C.
//!
//! Distinct from [`crate::kernel`] (which is the pure-data
//! minting and composition layer) and [`crate::capability`] (the
//! kernel model).

pub mod boot;
pub mod http_bridge;