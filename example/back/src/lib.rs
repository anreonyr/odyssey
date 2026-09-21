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
//! Builtins (after refactor):
//! - `agent` — observer (agent_list / agent_describe) + runtime
//!   (agent_start / resume / cancel / plan / stream /
//!   memory_recall / memory_record / load). The runtime's
//!   `memory` is internal; the agent requires `generator` and
//!   `embedder` from `model/`.
//! - `echo` — streaming pass-through; the first end-to-end
//!   `Resource::open` demo.
//! - `database` — `Database` trait + `InMemoryDatabase` impl;
//!   future backends (file, sqlite) can be added by impl'ing
//!   the trait.
//! - `inspectors` — `inspector` (kernel shape) +
//!   `schema_inspector` (agent shape); two read-only cspace
//!   observers.
//! - `model/generator` — stub `Generator` impl.
//! - `model/embedder` — stub `Embedder` impl.
//! - `model/reranker` — stub `Reranker` impl.

pub mod agent;
pub mod bundles;
pub mod database;
pub mod echo;
pub mod inspectors;
pub mod model;
