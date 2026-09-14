//! Phase 8: re-export shim.
//!
//! The Phase 5 dispatch table (`simple`/`echo_chain`/`generator`/
//! `agent`) was keyed by the 11 runtime plugin modules in
//! `crate::plugins::*`, which are being deleted in a later
//! commit. In the new architecture, the personality layer's
//! `mint_all` (in `personality::lifecycle::mint`) takes a
//! caller-supplied closure that builds the typed `Resource` for
//! each `(plugin, capability_decl)` pair. The workspace-member
//! `builtins/` crate supplies those closures.
//!
//! This module is kept as an empty shim so the legacy path
//! `crate::runtime::mint::mint_runtime_plugins` keeps compiling
//! for any out-of-tree code that may still import it during
//! the migration window. New code should use
//! `odyssey::personality::lifecycle::mint` directly.
