//! Built-in capability plugins for odyssey.
//!
//! Phase 8 re-creates three of the Phase 5 `plugins/` deleted
//! in commit 12 — the simplest ones, kept as workspace-member
//! builtins so the library stays clean of any concrete plugin
//! code.
//!
//! Each builtin exports:
//! - `manifest()` — a typed `PluginManifest` for the personality
//!   layer to resolve and mint.
//! - `mint()` — constructs the typed `Resource` handler that
//!   gets wrapped into a `Capability<R>` by the personality
//!   factory.
//!
//! Builtins:
//! - `agent` — read-only view over a plugin's reachable
//!   capabilities; the first consumer of the resolver's binding
//!   table.
//! - `echo` — pass-through (returns input verbatim).
//! - `reverse` — string reverse.
//! - `database` — key-value store with `get` / `set` / `delete`
//!   operations.
//! - `streaming_echo` — emits `count` chunks then `Done`; the
//!   first end-to-end `Resource::open` demo.

pub mod agent;
pub mod database;
pub mod echo;
pub mod reverse;
pub mod streaming_echo;
