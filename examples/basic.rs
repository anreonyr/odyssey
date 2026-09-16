//! Odyssey basic example — boots the capability kernel + HTTP bridge.
//!
//! Phase 8: this example binary lives at `examples/basic.rs`
//! (not in the library binary) to break the cyclic dependency
//! between the odyssey library and the `odyssey-builtins`
//! workspace member. Builtins depend on odyssey (the library);
//! the example depends on BOTH. The library itself stays
//! plugin-free.
//!
//! Phase 10/11: the registry of `(manifest, mint_fn, ruin_fn)`
//! triples is built here at boot from the colocated
//! `XBuiltin::register()` helpers in `builtins/src/`. Adding
//! a new builtin means adding one `XBuiltin::register()`
//! entry here and one module to `builtins/src/`; the
//! orchestrator's `run` doesn't change. Every `RuinFn` is
//! the default (just `cspace.revoke_tree` per slot) — no
//! builtin ships a custom teardown yet.
//!
//! The order matters for the resolver: provider plugins first
//! (the four original tool caps), then the readers/inspectors
//! (`agent_list` / `agent_describe` / `tool_descriptor` /
//! `profile_inspector`), then the LLM and memory providers,
//! and finally `agent_runtime` whose `requires` reference all
//! four providers. The orchestrator's resolver computes a
//! topological order; listing them in roughly that order keeps
//! the example's list match the actual mint order it produces.

use odyssey::personality::lifecycle::run::{DEFAULT_BRIDGE_ADDR, run_on};
use odyssey_builtins::{
    agent, agent_runtime, database, echo, llm, memory, profile_inspector, reverse, streaming_echo,
    tool_descriptor,
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
    run_on(
        addr.parse().expect("ODYSSEY_ADDR must be host:port"),
        &plugins,
    )
    .await
}
