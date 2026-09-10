//! Resolve-time errors. Phase 5 split from the monolithic
//! `resolver::mod` so the top-level resolver reads as a thin
//! orchestrator: index → topo → plan.

use crate::kernel::ids::PluginId;

#[derive(Debug)]
pub enum ResolveError {
    /// A consumer's `[[requires]] contract` had no matching
    /// `[[exposes]] contract_name` in any other manifest.
    Unprovided { contract: String, by: String },
    /// Two providers published the same contract without a
    /// priority hint; the resolver can't pick one.
    Ambiguous { contract: String, a: String, b: String },
    /// The dependency graph has a cycle. The chain lists the
    /// plugins that form the cycle, in `"name@version"` form.
    Cycle { chain: Vec<String> },
    DuplicateName { plugin: PluginId },
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unprovided { contract, by } => write!(
                f,
                "no provider for contract `{contract}` (requested by {by})"
            ),
            Self::Ambiguous { contract, a, b } => write!(
                f,
                "ambiguous contract `{contract}` (providers: {a}, {b})"
            ),
            Self::Cycle { chain } => write!(
                f,
                "dependency cycle detected ({} plugin(s))",
                chain.len()
            ),
            Self::DuplicateName { plugin } => write!(
                f,
                "duplicate plugin name `{}@{}`",
                plugin.name, plugin.version
            ),
        }
    }
}

impl std::error::Error for ResolveError {}
