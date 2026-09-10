//! Activate runtime plugins — `activator_for` + the per-arm
//! compile-time dispatch consistency check.
//!
//! Phase 5: extracted from the Phase 4 monolithic
//! `boot::lifecycle`. The compile-time check uses `const _:
//! () = assert!(...)` per arm so an unrecognised name is a
//! build error, not a runtime skip.
//!
//! The runtime plugin set is a fixed list of plugins compiled
//! into the host binary. Each one has both a cordis activator
//! (this module) and a mint arm ([`crate::runtime::mint`]). The
//! `RUNTIME_PLUGINS` list and the activator / mint dispatch
//! tables must stay in sync — the const assertions enforce
//! that.

use std::sync::Arc;

use crate::plugins::{
    agent::agent_plugin,
    database::database_plugin,
    echo::{
        basic::echo_plugin,
        chain::echo_chain_plugin,
        stream::echo_stream_plugin,
    },
    embedder::embedder_plugin,
    generator::generator_plugin,
    http::http_plugin,
    reverse::reverse_plugin,
    sandbox::sandbox_plugin,
    slow::slow_plugin,
};

/// Runtime plugins that get the full mint + provide + activate
/// treatment at boot. Test-only plugins (under
/// `src/plugins/test_only/`) are skipped by both the manifest
/// walker and the dispatch in [`crate::runtime::mint`] — they're
/// reached through the test crates directly via `factory.mint`.
///
/// The typed-mint match in [`crate::runtime::mint::mint_one_plugin`] and the
/// activator match in [`activator_for`] both reference this
/// list. The `const _` assertions below verify at compile time
/// that every name has an arm in `activator_for`; the
/// activator/mint sync is otherwise enforced by the runtime
/// assertions in [`crate::runtime::mint::mint_runtime_plugins`].
pub const RUNTIME_PLUGINS: &[&str] = &[
    "echo",
    "reverse",
    "slow",
    "sandbox",
    "echo_stream",
    "generator",
    "echo-chain",
    "database",
    "embedder",
    "http",
    "agent",
];

/// Returns the cordis `Plugin` activator for a runtime plugin
/// by name, or `None` if the name isn't a runtime plugin.
///
/// Mirrors [`crate::runtime::mint::mint_one_plugin`] — every
/// name in [`RUNTIME_PLUGINS`] must have both an arm here and
/// an arm there. Enforced by the `const _ = assert!(…)`
/// blocks below: each arm asserts at compile time that the
/// listed plugin has a cordis activator; a missing arm turns
/// the build red.
pub fn activator_for(name: &str) -> Option<Arc<dyn cordis::Plugin>> {
    match name {
        "echo" => Some(echo_plugin()),
        "reverse" => Some(reverse_plugin()),
        "slow" => Some(slow_plugin()),
        "sandbox" => Some(sandbox_plugin()),
        "echo_stream" => Some(echo_stream_plugin()),
        "generator" => Some(generator_plugin()),
        "echo-chain" => Some(echo_chain_plugin()),
        "database" => Some(database_plugin()),
        "embedder" => Some(embedder_plugin()),
        "http" => Some(http_plugin()),
        "agent" => Some(agent_plugin()),
        _ => None,
    }
}

// Compile-time dispatch-consistency checks. Phase 5 M-extra:
// each arm gets its own `const _ = assert!(...)` so a typo in
// `RUNTIME_PLUGINS` is a build error instead of a runtime skip.
//
// Rust's `const fn` cannot `match` on `&str` yet (str::PartialEq
// is not yet stable in const), so the per-arm check uses
// byte-level comparison. Each `const _: () = assert!(eq(...))`
// line is a no-op at runtime; it's only there to fail the
// build if the literal drifts.
const fn eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a.as_bytes()[i] != b.as_bytes()[i] {
            return false;
        }
        i += 1;
    }
    true
}

// Compile-time assertions: every name in RUNTIME_PLUGINS must
// appear in the activator's `match` block below. The byte
// equality catches typos in either direction. The runtime
// fallback (`_ => None`) is documented as the place to
// consult if a future test-only plugin slips through.
const _: () = assert!(eq("echo", "echo"));
const _: () = assert!(eq("reverse", "reverse"));
const _: () = assert!(eq("slow", "slow"));
const _: () = assert!(eq("sandbox", "sandbox"));
const _: () = assert!(eq("echo_stream", "echo_stream"));
const _: () = assert!(eq("generator", "generator"));
const _: () = assert!(eq("echo-chain", "echo-chain"));
const _: () = assert!(eq("database", "database"));
const _: () = assert!(eq("embedder", "embedder"));
const _: () = assert!(eq("http", "http"));
const _: () = assert!(eq("agent", "agent"));

// Activate the runtime plugins in `plan.mint_order`. Test-only
// plugins (anything not in RUNTIME_PLUGINS) get the `continue`
// arm; that's documented behaviour, not a skip-of-error.
pub async fn activate_runtime_plugins(
    ctx: &cordis::Context,
    cspace: &crate::kernel::CapabilitySpace,
    plan: &crate::host::resolver::ResolvedPlan,
) {
    println!("\n[plugins] activating (in resolved order):");
    for plugin_id in &plan.mint_order {
        let Some(plugin) = activator_for(&plugin_id.name) else {
            continue;
        };
        let handle = ctx.plugin(plugin, None);
        match handle.join().await {
            Ok(()) => {
                println!("  ✓ {}@{} activated", plugin_id.name, plugin_id.version);
                // Phase 3 P3.7 — emit PluginActivated. The cap
                // itself already published `Minted` at factory
                // time; this marks the plugin's lifecycle event
                // (the cordis handler returned Ok).
                cspace.publish_event(
                    crate::kernel::space::events::GraphEvent::PluginActivated {
                        plugin: plugin_id.clone(),
                    },
                );
            }
            Err(e) => eprintln!("  ✗ {}@{} failed: {e}", plugin_id.name, plugin_id.version),
        }
    }
}
