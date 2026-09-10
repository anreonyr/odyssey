//! 8-phase boot orchestration phases.
//!
//! Each phase is a thin call into a dedicated module; the
//! orchestrator in `mod.rs` is the only place that strings them
//! together.
//!
//! Phase 1  Load manifests → manifests::load_manifests
//! Phase 2  Provide core services (cspace, factory)
//! Phase 3  Resolve capability graph → crate::host::resolver
//! Phase 4  Mint runtime plugins → crate::runtime::mint
//! Phase 5  (reserved — was legacy consumes log; now no-op)
//! Phase 6  Activate runtime plugins → crate::runtime::activate
//! Phase 7  HTTP bridge + wait → shutdown::spawn_http_bridge
//! Phase 8  Teardown in reverse mint order → crate::runtime::teardown
