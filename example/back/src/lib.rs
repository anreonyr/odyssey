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
//!   table. Now also exposes the seven AI-agent caps
//!   (start / resume / cancel / plan / stream /
//!   memory_recall / memory_record).
//! - `echo` — pass-through (returns input verbatim).
//! - `reverse` — string reverse.
//! - `database` — key-value store with `get` / `set` / `delete`
//!   operations.
//! - `streaming_echo` — emits `count` chunks then `Done`; the
//!   first end-to-end `Resource::open` demo.
//! - `llm` — mock LLM provider; exposes `llm_complete` and
//!   `llm_embed`.
//! - `memory` — in-process memory backend; exposes `memory_query`
//!   and `memory_insert`.
//! - `tool_descriptor` — reads a cap's `tool_schema` field.
//! - `profile_inspector` — reads a cap's full `CapabilityMeta`.

pub mod agent;
pub mod agent_runtime;
pub mod database;
pub mod echo;
pub mod llm;
pub mod memory;
pub mod profile_inspector;
pub mod reverse;
pub mod streaming_echo;
pub mod tool_descriptor;
