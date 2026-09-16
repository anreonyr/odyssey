//! Personality / composition — pure computation step.
//!
//! Computation (planner phase). The `resolve` function turns
//! a flat list of `PluginManifest`s into a `ResolvedPlan`:
//! the topological order in which plugins must mint their
//! capabilities, and the per-plugin binding table.

pub mod resolve;
