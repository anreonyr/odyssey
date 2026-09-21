//! Odyssey basic example — boots the capability kernel + HTTP bridge
//! and serves the React app from `example/fore/dist`.
//!
//! The example binary lives at `example/backend/` to break the
//! cyclic dependency between the odyssey library and the
//! `odyssey-builtin` workspace member. Builtins depend on odyssey
//! (the library); the example depends on both. The library itself
//! stays plugin-free.
//!
//! The order matters for the resolver: tool plugins first
//! (the agent calls them at runtime), then the read-only
//! observers (agent_list / agent_describe / inspectors), then
//! the model providers (generator / embedder / reranker), and
//! finally `agent` whose `requires` reference generator and
//! embedder. The orchestrator's resolver computes a topological
//! order; listing them in roughly that order keeps the example's
//! list match the actual mint order it produces.
//!
//! After the refactor: 10 builtin manifests (down from 12).
//! Memory is internal to `agent`; no separate `memory` plugin.
//! The `llm` plugin became three independent stubs (`generator`,
//! `embedder`, `reranker`).

use std::path::PathBuf;

use odyssey::personality::lifecycle::run::{DEFAULT_BRIDGE_ADDR, run_on};
use odyssey_builtin::{
    agent, database, echo, inspectors,
    model::{embedder, generator, reranker},
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let plugins = vec![
        // Tool caps the agent can call.
        echo::EchoBuiltin::register(),
        database::DatabaseBuiltin::register(),
        // Read-only observers (binding-table based).
        agent::AgentListBuiltin::register(),
        agent::AgentDescribeBuiltin::register(),
        // Cspace inspectors (read-only).
        inspectors::ProfileInspectorBuiltin::register(),
        inspectors::SchemaInspectorBuiltin::register(),
        // Model providers (no dependencies on each other;
        // required by agent).
        generator::GeneratorBuiltin::register(),
        embedder::EmbedderBuiltin::register(),
        reranker::RerankerBuiltin::register(),
        // The agent — requires generator and embedder;
        // calls tool caps via cspace lookup at runtime,
        // gated by per-session `allowed_tools`.
        agent::AgentRuntimeBuiltin::register(),
    ];
    // `ODYSSEY_ADDR` lets a test give its own instance a port of
    // its own; without it the smoke test competes for the fixed one.
    let addr = std::env::var("ODYSSEY_ADDR").unwrap_or_else(|_| DEFAULT_BRIDGE_ADDR.to_string());

    // Frontend dist is one level up from the binary's manifest
    // dir: `example/back/` → `example/fore/dist`. The library's
    // serve reads from disk so we don't embed HTML; we just
    // point it at the build artefact.
    let frontend_dist: Option<PathBuf> = std::env::var("ODYSSEY_NO_FRONTEND")
        .ok()
        .map(|_| None)
        .unwrap_or_else(|| {
            Some(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .expect("example/backend has a parent")
                    .join("fore/dist"),
            )
        });

    run_on(
        addr.parse().expect("ODYSSEY_ADDR must be host:port"),
        frontend_dist.as_deref(),
        &plugins,
    )
    .await
}
