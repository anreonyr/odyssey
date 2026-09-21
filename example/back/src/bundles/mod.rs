//! Builtin bundles — named groupings of the example's builtin
//! plugins.
//!
//! The kernel knows nothing about bundles at runtime
//! (`run_on` still receives a flat
//! `&[(PluginManifest, MintFn, RuinFn)]` slice — see
//! `crate/odyssey/src/personality/lifecycle/run.rs:155`). The
//! "bundle" concept exists at this layer for two purposes:
//!
//! 1. Resolver output grouping — `ResolvedPlan::render` reads
//!    each manifest's `bundle` field and groups the mint
//!    order by `BundleId` in the boot diagram
//!    (`crate/odyssey/src/personality/composition/resolve.rs`).
//! 2. Error attribution — `ResolveError` variants carry a
//!    `requester_bundle: Option<BundleId>` field so a boot
//!    failure points at the bundle that contained the
//!    offending plugin.
//!
//! Each bundle function stamps its member manifests with a
//! `BundleId` *after* `Builtin::register()` returns, by
//! writing to the manifest's public `bundle` field. This
//! keeps every builtin's `register()` signature unchanged
//! (still `() -> (PluginManifest, MintFn, RuinFn)`) — the
//! stamp is the bundle layer's job.
//!
//! ## Grouping
//!
//! Five bundles, partitioned by capability kind:
//!
//! - **tool-caps** (`echo`, `database`) — caps the agent
//!   invokes at runtime.
//! - **observers** (`agent_list`, `agent_describe`) —
//!   read-only views over the resolver's binding table.
//! - **inspectors** (`inspector`, `schema_inspector`) —
//!   read-only cspace inspectors.
//! - **model-providers** (`generator`, `embedder`,
//!   `reranker`) — stubs the agent runtime requires.
//! - **agent** (`agent`) — the AI runtime; requires
//!   `generator` and `embedder` from `model-providers`.
//!
//! The grouping matches the documented dependency intent:
//! tool caps and observers read from the tool caps;
//! inspectors read the cspace; the model providers feed the
//! agent. The resolver's topological sort still produces a
//! flat mint order — the grouping is purely a display
//! affordance.

use odyssey::personality::lifecycle::run::{MintFn, RuinFn};

use odyssey::core::manifest::BundleId;
use odyssey::core::manifest::manifest::PluginManifest;

// Sibling modules in the same crate (`odyssey_builtin` per
// `[lib] name` in Cargo.toml). `crate::*` resolves to the
// crate's root because `bundles/` is a submodule of
// `src/lib.rs`.
use crate::model::{embedder, generator, reranker};
use crate::{agent, bridge, database, echo, inspectors};

/// Stamp every member manifest in `entries` with `id` and
/// return the stamped slice. The orchestrator consumes the
/// stamped manifests unchanged; the stamp exists only so
/// `ResolvedPlan::render` and the resolve-error variants
/// can attribute output to a bundle.
///
/// `BundleId` is `Clone`; we clone per entry because each
/// manifest owns its `bundle: Option<BundleId>` field
/// (precedent `064cada` — no mirror state, manifest is
/// the single source of truth).
fn stamp(
    id: BundleId,
    entries: Vec<(PluginManifest, MintFn, RuinFn)>,
) -> Vec<(PluginManifest, MintFn, RuinFn)> {
    entries
        .into_iter()
        .map(|(mut m, mint, ruin)| {
            m.bundle = Some(id.clone());
            (m, mint, ruin)
        })
        .collect()
}

/// Tool caps the agent invokes at runtime.
///
/// Members: `echo`, `database`. Both publish tool caps the
/// `agent_runtime` builtin calls into via cspace lookup at
/// runtime, gated by per-session `allowed_tools`. No
/// `requires`; both are roots.
pub fn tool_caps() -> Vec<(PluginManifest, MintFn, RuinFn)> {
    stamp(
        BundleId::with_default_version("tool-caps"),
        vec![
            echo::EchoBuiltin::register(),
            database::DatabaseBuiltin::register(),
        ],
    )
}

/// Read-only observers over the resolver's binding table.
///
/// Members: `agent_list`, `agent_describe`. The agent
/// runtime uses both to enumerate its reachable caps and
/// describe a specific cap. `agent_describe` requires
/// `echo` and `database` (cross-bundle `requires` from
/// `observers` into `tool-caps`) — the resolver's flat
/// flatten handles this transparently.
pub fn observers() -> Vec<(PluginManifest, MintFn, RuinFn)> {
    stamp(
        BundleId::with_default_version("observers"),
        vec![
            agent::AgentListBuiltin::register(),
            agent::AgentDescribeBuiltin::register(),
        ],
    )
}

/// Read-only cspace inspectors.
///
/// Members: `inspector` (kernel-shape cap introspection:
/// `id`, `namespace`, `contract`, `kind`, `budget`,
/// `operations`), `schema_inspector` (agent-shape
/// introspection: `input_schema`, `output_schema`,
/// `description`). Both are read-only peers — no
/// `requires`.
pub fn inspectors_bundle() -> Vec<(PluginManifest, MintFn, RuinFn)> {
    stamp(
        BundleId::with_default_version("inspectors"),
        vec![
            inspectors::ProfileInspectorBuiltin::register(),
            inspectors::SchemaInspectorBuiltin::register(),
        ],
    )
}

/// Model providers — stubs the agent runtime requires.
///
/// Members: `generator`, `embedder`, `reranker`. The first
/// two are required by `agent` (cross-bundle `requires`
/// from `agent` into `model-providers`); `reranker` is a
/// standalone stub for future use. All three are leaves.
pub fn model_providers() -> Vec<(PluginManifest, MintFn, RuinFn)> {
    stamp(
        BundleId::with_default_version("model-providers"),
        vec![
            generator::GeneratorBuiltin::register(),
            embedder::EmbedderBuiltin::register(),
            reranker::RerankerBuiltin::register(),
        ],
    )
}

/// The AI agent runtime.
///
/// Single member: `agent`. Requires `generator` and
/// `embedder` from `model-providers` — the resolver's
/// topological sort places `agent` last because both
/// providers must mint before it can bind to them.
pub fn agent_bundle() -> Vec<(PluginManifest, MintFn, RuinFn)> {
    stamp(
        BundleId::with_default_version("agent"),
        vec![agent::AgentRuntimeBuiltin::register()],
    )
}

/// HTTP bridge plugin — exposes the `http_bridge`
/// capability whose Resource holds the spawned axum
/// server. Single member, no `requires`.
///
/// The bundle exists for grouping + error attribution
/// (`ResolvedPlan::render` groups mint order by
/// `BundleId`). The bridge mints among the leaves in
/// topological lex order.
pub fn bridge_bundle() -> Vec<(PluginManifest, MintFn, RuinFn)> {
    stamp(
        BundleId::with_default_version("bridge"),
        vec![bridge::HttpBridgeBuiltin::register()],
    )
}

#[cfg(test)]
mod tests {
    //! The bundle functions stamp every member manifest with
    //! the same `BundleId`, and the stamps have the same
    //! name as the bundle function name + the default
    //! version `"0.1.0"`.

    use super::*;

    fn assert_all_stamped(entries: &[(PluginManifest, MintFn, RuinFn)], expected_name: &str) {
        assert!(!entries.is_empty(), "bundle must have at least one entry");
        for (m, _, _) in entries {
            let bundle = m
                .bundle
                .as_ref()
                .unwrap_or_else(|| panic!("manifest `{}` was not stamped", m.plugin.name));
            assert_eq!(bundle.name, expected_name);
            assert_eq!(bundle.version, "0.1.0");
        }
    }

    #[test]
    fn tool_caps_stamps_echo_and_database() {
        let entries = tool_caps();
        assert_eq!(entries.len(), 2);
        assert_all_stamped(&entries, "tool-caps");
        let names: Vec<&str> = entries
            .iter()
            .map(|(m, _, _)| m.plugin.name.as_str())
            .collect();
        assert!(names.contains(&"echo"));
        assert!(names.contains(&"database"));
    }

    #[test]
    fn observers_stamps_agent_list_and_agent_describe() {
        let entries = observers();
        assert_eq!(entries.len(), 2);
        assert_all_stamped(&entries, "observers");
        let names: Vec<&str> = entries
            .iter()
            .map(|(m, _, _)| m.plugin.name.as_str())
            .collect();
        assert!(names.contains(&"agent_list"));
        assert!(names.contains(&"agent_describe"));
    }

    #[test]
    fn inspectors_bundle_stamps_inspector_and_schema_inspector() {
        let entries = inspectors_bundle();
        assert_eq!(entries.len(), 2);
        assert_all_stamped(&entries, "inspectors");
        let names: Vec<&str> = entries
            .iter()
            .map(|(m, _, _)| m.plugin.name.as_str())
            .collect();
        assert!(names.contains(&"inspector"));
        assert!(names.contains(&"schema_inspector"));
    }

    #[test]
    fn model_providers_stamps_generator_embedder_reranker() {
        let entries = model_providers();
        assert_eq!(entries.len(), 3);
        assert_all_stamped(&entries, "model-providers");
        let names: Vec<&str> = entries
            .iter()
            .map(|(m, _, _)| m.plugin.name.as_str())
            .collect();
        assert!(names.contains(&"generator"));
        assert!(names.contains(&"embedder"));
        assert!(names.contains(&"reranker"));
    }

    #[test]
    fn agent_bundle_stamps_agent() {
        let entries = agent_bundle();
        assert_eq!(entries.len(), 1);
        assert_all_stamped(&entries, "agent");
        assert_eq!(entries[0].0.plugin.name, "agent");
    }

    #[test]
    fn all_bundles_total_eleven_builtins() {
        let total = tool_caps().len()
            + observers().len()
            + inspectors_bundle().len()
            + model_providers().len()
            + agent_bundle().len()
            + bridge_bundle().len();
        // Matches the 11 manifests shipped by the example
        // after the bridge plugin lands (10 builtins + 1 bridge).
        assert_eq!(total, 11);
    }

    #[test]
    fn bridge_bundle_stamps_http_bridge() {
        let entries = bridge_bundle();
        assert_eq!(entries.len(), 1);
        assert_all_stamped(&entries, "bridge");
        assert_eq!(entries[0].0.plugin.name, "http_bridge");
    }
}
