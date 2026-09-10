//! Echo family plugins — `src/plugins/echo/`.
//!
//! Three plugins share the "echo" identity: a passthrough that
//! returns its input verbatim. They differ in shape so each one
//! exercises a different capability-system axis.
//!
//! - [`basic`] — sync, in-process, no dependencies. The simplest
//!   capability possible: receive JSON, return it.
//! - [`chain`] — sync, depends on `basic` (Phase 3 P3.1
//!   `[[requires]]` declaration). Wraps a typed
//!   `Capability<EchoResource>` at mint time and produces a
//!   chained envelope on invoke.
//! - [`stream`] — streaming (Channel-style `mpsc::Receiver`
//!   response). Demonstrates the `StreamKind` capability kind
//!   and the chunk-based delivery path. Module name is
//!   `stream` (sibling to `basic` and `chain`); the plugin
//!   identifier in the manifest is `echo_stream`.
//!
//! Plugin identifiers in the `.toml` manifests are
//! `echo`, `echo-chain`, and `echo_stream` — all sharing the
//! `echo_` prefix so the family is greppable.

pub mod basic;
pub mod chain;
pub mod stream;
