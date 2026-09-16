//! Odyssey basic example — boots the capability kernel + HTTP bridge
//! and serves the React app from `example/fore/dist`.
//!
//! The example binary lives at `example/backend/` to break the
//! cyclic dependency between the odyssey library and the
//! `odyssey-builtin` workspace member. Builtins depend on odyssey
//! (the library); the example depends on both. The library itself
//! stays plugin-free.
//!
//! The order matters for the resolver: provider plugins first
//! (the four original tool caps), then the readers/inspectors
//! (`agent_list` / `agent_describe` / `tool_descriptor` /
//! `profile_inspector`), then the LLM and memory providers,
//! and finally `agent_runtime` whose `requires` reference all
//! four providers. The orchestrator's resolver computes a
//! topological order; listing them in roughly that order keeps
//! the example's list match the actual mint order it produces.
//!
//! Every `RuinFn` is the default (just `cspace.revoke_tree` per
//! slot) — no builtin ships a custom teardown yet. Adding a new
//! builtin means adding one `XBuiltin::register()` entry here
//! and one module to `odyssey-builtin/src/`; the orchestrator's
//! `run` doesn't change.

use std::path::PathBuf;

use odyssey::personality::lifecycle::run::{DEFAULT_BRIDGE_ADDR, run_on};
use odyssey_builtin::{
    agent, agent_runtime, database, echo, llm, memory, profile_inspector, reverse,
    streaming_echo, tool_descriptor,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let plugins = vec![
        // Tool caps the agent can call.
        echo::EchoBuiltin::register(),
        reverse::ReverseBuiltin::register(),
        database::DatabaseBuiltin::register(),
        streaming_echo::StreamingEchoBuiltin::register(),
        // Read-only observers (binding-table based).
        agent::AgentListBuiltin::register(),
        agent::AgentDescribeBuiltin::register(),
        // Cspace inspectors (read-only).
        tool_descriptor::ToolDescriptorBuiltin::register(),
        profile_inspector::ProfileInspectorBuiltin::register(),
        // LLM + memory providers (no dependencies on each other
        // or on the tool caps; required by agent_runtime).
        llm::LlmBuiltin::register(),
        memory::MemoryBuiltin::register(),
        // The AI agent runtime — requires the LLM and memory
        // caps; calls the tool caps via cspace lookup at
        // runtime, gated by per-session `allowed_tools`.
        agent_runtime::AgentRuntimeBuiltin::register(),
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
