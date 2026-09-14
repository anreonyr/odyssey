//! Phase 8: re-export shim.
//!
//! The 8-phase orchestrator that used to live here (`run()`) has
//! moved to `personality::lifecycle::run`. The phase files
//! (`manifests`, `print`, `phases`, `shutdown`) all moved too
//! (manifests+print into `personality::lifecycle::boot`,
//! shutdown into `personality::lifecycle::serve`, phases was
//! dead doc and was deleted).
//!
//! This module is kept as a shim so legacy paths
//! (`crate::runtime::lifecycle::run`) keep compiling during the
//! migration window. New code should use
//! `odyssey::personality::lifecycle::run` directly.
