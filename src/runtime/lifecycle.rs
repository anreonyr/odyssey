//! 7-phase boot orchestrator — load manifests, mint
//! runtime plugins, activate, bring up the HTTP bridge,
//! wait for ctrl-C, tear down.
//!
//! Phase 5: split from the Phase 4 monolithic `boot::lifecycle`.
//! Each phase is now a thin call into a dedicated module:
//!
//!   Phase 1  Load manifests → load_manifests (this file)
//!   Phase 2  Provide core services
//!   Phase 3  Resolve capability graph → crate::host::resolver
//!   Phase 4  Mint runtime plugins → crate::runtime::mint
//!   Phase 5  Log legacy `consumes` (informational)
//!   Phase 6  Activate runtime plugins → crate::runtime::activate
//!   Phase 7  HTTP bridge + wait → crate::runtime::http_bridge
//!   Phase 8  Teardown in reverse mint order → crate::runtime::teardown
//!
//! Phase 7 in earlier versions exercised the caps with a demo
//! transcript; that has moved to `tests/{alpha,beta,gamma,delta,
//! epsilon,zeta}/`, so the boot here stops at "kernel +
//! plugins running".

use crate::host::factory::CapabilityFactory;
use crate::host::manifest::PluginManifest;
use crate::host::resolver::{resolve, ResolvedPlan};
use crate::kernel::space::events::GraphEvent;
use crate::kernel::CapabilitySpace;
use crate::runtime::http_bridge::serve;

const MANIFEST_DIR: &str = "src/plugins";

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Phase 1: Parse manifests.
    let manifests = load_manifests(MANIFEST_DIR)?;
    print_manifests(&manifests);

    // Phase 2: Provide core services.
    let ctx = cordis::Context::new();
    let cspace = CapabilitySpace::new();
    let factory = CapabilityFactory::new(cspace.clone());

    ctx.provide("capability_space", cspace.clone()).await?;
    ctx.provide("capability_factory", factory.clone()).await?;
    println!("[main] core services provided");

    // Phase 3: Resolve the capability dependency graph.
    let plan: ResolvedPlan = resolve(&manifests).map_err(|e| format!("resolver: {e}"))?;
    println!("\n[resolver]\n{}", plan.render());

    // Phase 4: Mint runtime plugins in resolved order.
    println!("[mint] runtime plugins (in resolved order):");
    let minted = crate::runtime::mint::mint_runtime_plugins(
        &ctx, &factory, &cspace, &plan, &manifests,
    )
    .await?;

    // Phase 5: legacy `consumes` log (informational only).
    log_legacy_consumes(&manifests);

    // Phase 6: Activate runtime plugins in resolved order.
    crate::runtime::activate::activate_runtime_plugins(&ctx, &cspace, &plan).await;

    // Phase 7: HTTP bridge + wait.
    let cspace_clone = cspace.clone();
    let server_handle = tokio::spawn(async move {
        serve(
            "127.0.0.1:3030".parse().unwrap(),
            cspace_clone,
            async {
                let _ = tokio::signal::ctrl_c().await;
            },
        )
        .await;
    });
    eprintln!("\n[main] HTTP bridge up — open http://127.0.0.1:3030/");
    eprintln!("[main] press Ctrl-C to stop");

    let _ = server_handle.await;
    eprintln!("[main] shutting down");

    // Phase 8 (P3.6 + P3.7): tear down runtime plugins in
    // reverse mint order.
    cspace.publish_event(GraphEvent::ShutdownStarted);
    crate::runtime::teardown::ruin_runtime_plugins(&cspace, &plan, &minted).await;
    cspace.publish_event(GraphEvent::ShutdownCompleted {
        remaining_slots: cspace.len(),
    });

    ctx.stop().await;

    Ok(())
}

/// Collect every runtime plugin's manifest.
///
/// Phase 3 replaces the toml walker with explicit enumeration
/// of each plugin's `manifest()` function. The toml files for
/// runtime plugins no longer exist; test_only plugins are
/// reached by the test crates directly via `factory.mint`, not
/// through boot.
///
/// The `dir` parameter is preserved as a positional argument
/// for callers that pass it; the loader walks the workspace
/// from `CARGO_MANIFEST_DIR` directly and ignores the value.
fn load_manifests(_dir: &str) -> Result<Vec<PluginManifest>, Box<dyn std::error::Error>> {
    use crate::plugins::{
        agent::manifest as agent_manifest,
        database::manifest as database_manifest,
        echo::{
            basic::manifest as echo_basic_manifest,
            chain::manifest as echo_chain_manifest,
            stream::manifest as echo_stream_manifest,
        },
        embedder::manifest as embedder_manifest,
        generator::manifest as generator_manifest,
        http::manifest as http_manifest,
        reverse::manifest as reverse_manifest,
        sandbox::manifest as sandbox_manifest,
        slow::manifest as slow_manifest,
    };

    let manifests = [
        database_manifest(),
        embedder_manifest(),
        echo_basic_manifest(),
        echo_chain_manifest(),
        echo_stream_manifest(),
        agent_manifest(),
        generator_manifest(),
        http_manifest(),
        reverse_manifest(),
        sandbox_manifest(),
        slow_manifest(),
    ];
    let mut out: Vec<PluginManifest> = manifests.iter().map(|m| (*m).clone()).collect();
    out.sort_by(|a, b| a.plugin.name.cmp(&b.plugin.name));
    Ok(out)
}

/// Print every loaded manifest so operators can confirm the
/// resolver's input.
fn print_manifests(manifests: &[PluginManifest]) {
    let mut rows: Vec<(String, String, String, String)> = Vec::with_capacity(manifests.len());
    for m in manifests {
        let name_ver = format!("{}@{}", m.plugin.name, m.plugin.version);
        let exposes = m
            .exposes
            .iter()
            .map(|c| c.name.clone())
            .collect::<Vec<_>>()
            .join(",");
        let contracts = m
            .exposes
            .iter()
            .map(|c| c.contract_name.clone())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(",");
        let requires = if m.requires.is_empty() {
            "—".to_string()
        } else {
            m.requires
                .iter()
                .map(|r| format!("{}→{}", r.name, r.contract))
                .collect::<Vec<_>>()
                .join(",")
        };
        rows.push((name_ver, exposes, contracts, requires));
    }

    let headers = (
        "plugin".to_string(),
        "exposes".to_string(),
        "contract".to_string(),
        "requires".to_string(),
    );
    let col_width = |label: &str, cells: &[String]| -> usize {
        cells
            .iter()
            .map(|s| s.chars().count())
            .max()
            .unwrap_or(0)
            .max(label.chars().count())
            + 1
    };
    let wp = col_width(&headers.0, &rows.iter().map(|r| r.0.clone()).collect::<Vec<_>>());
    let we = col_width(&headers.1, &rows.iter().map(|r| r.1.clone()).collect::<Vec<_>>());
    let wc = col_width(&headers.2, &rows.iter().map(|r| r.2.clone()).collect::<Vec<_>>());
    let wr = col_width(&headers.3, &rows.iter().map(|r| r.3.clone()).collect::<Vec<_>>());

    println!("[manifest] loaded {} plugin(s):", manifests.len());
    println!(
        "  {:<wp$}{:<we$}{:<wc$}{:<wr$}",
        headers.0, headers.1, headers.2, headers.3,
    );
    println!(
        "  {}{}{}{}",
        "-".repeat(wp - 1),
        "-".repeat(we),
        "-".repeat(wc),
        "-".repeat(wr),
    );
    for (n, e, c, r) in &rows {
        println!(
            "  {:<wp$}{:<we$}{:<wc$}{:<wr$}",
            n, e, c, r,
        );
    }
}

/// Print every legacy `consumes` entry as informational only;
/// the resolver walks `[[requires]]`, not `consumes`.
fn log_legacy_consumes(manifests: &[PluginManifest]) {
    println!(
        "\n[legacy] `consumes` entries (informational; resolver uses `requires`):"
    );
    let provided_caps: std::collections::HashSet<String> = manifests
        .iter()
        .flat_map(|m| m.exposes.iter().map(|c| c.name.clone()))
        .collect();
    let mut legacy_unprovided: Vec<String> = Vec::new();
    for m in manifests {
        for dep in &m.consumes {
            let ok = provided_caps.contains(&dep.capability);
            let mark = if ok { "✓" } else { "⚠" };
            println!(
                "  {} {}@{} consumes {} from {}@{} ({})",
                mark,
                m.plugin.name,
                m.plugin.version,
                dep.capability,
                dep.plugin,
                dep.version,
                if ok {
                    "provided"
                } else {
                    "unprovided — migrate to [[requires]]"
                }
            );
            if !ok {
                legacy_unprovided.push(format!(
                    "{} → {} (from {}@{})",
                    m.plugin.name, dep.capability, dep.plugin, dep.version
                ));
            }
        }
    }
    if !legacy_unprovided.is_empty() {
        println!(
            "[legacy] {} `consumes` entry/entries have no provider; boot continues \
             because the resolver uses `requires`. Consider migrating:\n  - {}",
            legacy_unprovided.len(),
            legacy_unprovided.join("\n  - ")
        );
    }
}
