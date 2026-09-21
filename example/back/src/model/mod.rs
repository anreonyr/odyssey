//! `model/` — three independent stub plugins:
//! - `generator` — `Generator` trait + `StubGenerator`.
//! - `embedder`  — `Embedder` trait + `StubEmbedder`.
//! - `reranker`  — `Reranker` trait + `StubReranker`.
//!
//! Each is its own plugin manifest (3 `BuiltinManifest`
//! impls); the directory is just for code organisation.

pub mod embedder;
pub mod generator;
pub mod reranker;
