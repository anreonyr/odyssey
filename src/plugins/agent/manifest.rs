//! Phase 4 P4.4 — Capability-Native Agent manifest.
//!
//! Exposes one capability, `agent`, with one action:
//!
//! | action | operation bit | operation                    |
//! | ------ | ------------- | ---------------------------- |
//! | `run`  | `AGENT_RUN`   | run program → stream events  |
//!
//! Streaming capability: each program step is one event
//! emitted on the `mpsc::Receiver<CapabilityChunk>`.
//!
//! ## `[[requires]]` — the agent's "expected environment"
//!
//! The agent declares the capabilities it expects to find in
//! its environment. At boot, the resolver computes bindings
//! for each: the agent's reachable set is whatever the env
//! actually provides.
//!
//! In the default boot, all four are present (echo, generate,
//! embed, database). In a restricted env (Phase 4 P4.5 /
//! P4.6), the resolver drops bindings for missing providers
//! and the agent's reachable shrinks — the SAME program then
//! produces different outcomes because steps go from `ok` to
//! `skip` / `deny`.

use std::sync::OnceLock;

use crate::host::manifest::PluginManifest;
use crate::host::manifest::ManifestBuilder;

static MANIFEST: OnceLock<PluginManifest> = OnceLock::new();

pub fn manifest() -> &'static PluginManifest {
    MANIFEST.get_or_init(|| {
        ManifestBuilder::new("agent", "agent", "agent")
            .in_type("program")
            .out_type("events")
            .streaming(true)
            .action("run", "AGENT_RUN")
            .requires("echo", "echo")
            .requires("generate", "generate")
            .requires("embed", "embed")
            .requires("database", "database")
            .timeout_ms(30000)
            .build()
    })
}
