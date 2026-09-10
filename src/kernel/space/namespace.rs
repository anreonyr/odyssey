//! Hierarchical namespace matching — single home for the prefix
//! algorithm used by `enumerate_namespace` and the graph view.
//!
//! Phase 5 R1 fix: previously this function lived twice, in
//! `cspace.rs::namespace_prefix_matches` and `graph.rs::namespace_starts_with`.
//! Both are now this one function.

/// True when `namespace` is filed under `prefix` in the hierarchical
/// namespace tree. The empty prefix matches everything; a non-empty
/// prefix matches its own node and every descendant.
pub fn namespace_prefix_matches(namespace: &str, prefix: &str) -> bool {
    if prefix.is_empty() || prefix == "." {
        return true;
    }
    if namespace == prefix {
        return true;
    }
    namespace.starts_with(&format!("{prefix}."))
}
