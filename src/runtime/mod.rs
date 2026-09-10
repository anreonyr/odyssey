//! Runtime adapter — boot lifecycle, HTTP bridge, mint dispatch.
//!
//! Phase 5 split from `src/boot`. The runtime layer is the
//! thin glue that:
//!
//! - Loads every manifest via the host loader.
//! - Resolves the capability dependency graph.
//! - Mints typed `Capability<R>` for each runtime plugin in
//!   resolved order.
//! - Activates plugins via cordis.
//! - Brings up the HTTP bridge and waits for Ctrl-C.
//! - Tears down runtime plugins in reverse mint order
//!   (P3.6), revoking each slot via `cspace.revoke_tree`.
//!
//! Submodules:
//!
//! - `mint` — typed `Capability<R>` mint helpers (one file
//!   per resource class: simple / echo-chain / generator / agent).
//! - `activate` — `activator_for` + the per-arm `const _`
//!   dispatch-consistency assertions.
//! - `teardown` — `ruin_runtime_plugins` (reverse mint order).
//! - `http_bridge` — the axum router + serve loop.
//! - `lifecycle` — the 7-phase orchestrator that calls into
//!   every other module.

pub mod activate;
pub mod http_bridge;
pub mod lifecycle;
pub mod mint;
pub mod teardown;
