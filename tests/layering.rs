//! Layering invariant test.
//!
//! Phase 8 split: the crate has three layers
//! (`core` / `capability` / `personality`) with a strict
//! dependency direction:
//!
//!     personality ──▶ capability ──▶ core
//!                     │            │
//!                     └────────────┘
//!
//! Each test below enforces one direction. `core` is a leaf and
//! must not reach into either sibling. `capability` must not
//! reach into `personality` (the kernel has no knowledge of
//! plugins or lifecycle). `personality` *may* reach into
//! `capability` (the typed mint dispatch landed here, not in
//! `core`, because the factory's generic `mint<R>` method made
//! a trait-object bridge too invasive for Phase 8).
//!
//! These invariants are enforced by reading every `.rs` file in
//! the three layer directories and checking for forbidden
//! `use crate::NAME::...` paths.

use std::fs;
use std::path::{Path, PathBuf};

/// Collect every `.rs` file under `dir` recursively.
fn collect_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(p) = stack.pop() {
        let entries = match fs::read_dir(&p) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| {
        panic!("read {}: {e}", path.display());
    })
}

/// Find every line that starts with `use` and references the
/// forbidden layer via `crate::FORBIDDEN::...`.
fn find_uses_of(content: &str, forbidden: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in content.lines() {
        let l = line.trim();
        if !(l.starts_with("use ") || l.starts_with("pub use ")) {
            continue;
        }
        // Look for `crate::FORBIDDEN` as a path segment.
        let needle = format!("crate::{forbidden}::");
        let needle_short = format!("crate::{forbidden}");
        // Allow `crate::FORBIDDEN` only when followed by `::` (it's
        // a sub-module), or when followed by a non-identifier char
        // (e.g. `;`). This avoids matching `crate::capability_xyz`.
        let mut idx = 0usize;
        while let Some(at) = l[idx..].find(&needle) {
            let abs = idx + at;
            let after = &l[abs + needle.len()..];
            // Either there's a `::` after, or it's at end-of-line /
            // followed by non-identifier (whitespace, `;`, `,`,
            // `{`, etc.).
            let ok = if after.is_empty() {
                true
            } else {
                let first = after.chars().next().unwrap();
                !first.is_alphanumeric() && first != '_'
            };
            if ok {
                out.push(format!("{l}"));
            }
            idx = abs + needle.len();
        }
        // Also handle the bare `crate::FORBIDDEN;` form (e.g.
        // `use crate::capability;`).
        if l.contains(&needle_short) {
            // Find the position and check what follows.
            if let Some(at) = l.find(&needle_short) {
                let after = &l[at + needle_short.len()..];
                let ok = if after.is_empty() {
                    true
                } else {
                    let first = after.chars().next().unwrap();
                    !first.is_alphanumeric() && first != '_'
                };
                if ok && !out.iter().any(|x| x == l) {
                    out.push(format!("{l}"));
                }
            }
        }
    }
    out
}

fn violations_in(layer_dir: &Path, forbidden_layer: &str) -> Vec<String> {
    let mut out = Vec::new();
    for path in collect_rs_files(layer_dir) {
        let content = read(&path);
        for v in find_uses_of(&content, forbidden_layer) {
            out.push(format!("{}: {v}", path.display()));
        }
    }
    out
}

#[test]
fn core_has_no_dependency_on_capability_or_personality() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"));
    let core_dir = workspace.join("src/core");
    assert!(core_dir.is_dir(), "src/core should exist");

    let mut all_violations = Vec::new();
    for forbidden in &["capability", "personality"] {
        all_violations.extend(violations_in(&core_dir, forbidden));
    }
    assert!(
        all_violations.is_empty(),
        "src/core/ must not import from capability or personality:\n  {}",
        all_violations.join("\n  ")
    );
}

#[test]
fn capability_has_no_dependency_on_personality() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"));
    let cap_dir = workspace.join("src/capability");
    assert!(cap_dir.is_dir(), "src/capability should exist");

    let violations = violations_in(&cap_dir, "personality");
    assert!(
        violations.is_empty(),
        "src/capability/ must not import from personality:\n  {}",
        violations.join("\n  ")
    );
}

#[test]
fn personality_depends_on_capability_through_typed_path() {
    // Sanity-check the dependency direction. personality depends
    // on capability (factory, cspace, budget). The other
    // direction is enforced by the previous test.
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"));
    let personality_dir = workspace.join("src/personality");

    let uses_capability = violations_in(&personality_dir, "capability");
    assert!(
        !uses_capability.is_empty(),
        "src/personality/ must depend on crate::capability (cspace, \
         factory, budget). Found no such imports — the \
         orchestrator has lost its connection to the kernel?"
    );
}
