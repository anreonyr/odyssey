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
//! These invariants are enforced by parsing every `.rs` file in
//! the three layer directories with `syn`, walking every `use`
//! tree, and flagging any path segment that names a forbidden
//! layer. Group imports, multi-line `use` statements, aliased
//! imports, and nested groups are all handled correctly.
//!
//! Phase 9.5 history: the previous version also pinned a
//! positive sanity check (`builtins_depend_on_personality_
//! for_typed_mint`) via a custom walker that matched the
//! three-segment `personality::lifecycle::mint` path. Two
//! consecutive fixes (`a0db405` + `bfccc47`) closed a latent
//! false-negative and the false-positive the fix opened —
//! shape matching on use-trees is brittle. The check is gone;
//! the integration is now covered by `tests/smoke.rs`, which
//! performs an actual mint + typed-slot + invoke round-trip
//! against `EchoBuiltin`. That test would fail at compile
//! time if the import path broke, and at runtime if any of
//! `Mint::mint` / `CapabilityFactory::with_clock` /
//! `Slot::new` / `Slot::invoke` broke — same contract, real
//! behaviour instead of text shape.

use std::fs;
use std::path::{Path, PathBuf};

use syn::UseTree;

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

fn find_uses(content: &str, forbidden: &str) -> Vec<String> {
    let ast = match syn::parse_file(content) {
        Ok(f) => f,
        // The layering test should never see unparseable input:
        // every `.rs` file in `src/` and `builtins/src/` is
        // compiled by `cargo build --workspace --examples`, so a
        // parse failure here is a bug in the test, not the
        // source. Panic loudly rather than silently skipping.
        Err(e) => panic!("syn parse_file failed: {e}"),
    };
    let mut out = Vec::new();
    for item in ast.items {
        if let syn::Item::Use(item_use) = item {
            collect_path_segments(&item_use.tree, forbidden, &mut out);
        }
    }
    out
}

fn collect_path_segments(tree: &UseTree, forbidden: &str, out: &mut Vec<String>) {
    match tree {
        UseTree::Path(p) => {
            if p.ident == forbidden {
                push_unique(out, forbidden.to_string());
            }
            collect_path_segments(&p.tree, forbidden, out);
        }
        UseTree::Name(n) => {
            if n.ident == forbidden {
                push_unique(out, forbidden.to_string());
            }
        }
        UseTree::Rename(r) => {
            if r.ident == forbidden {
                push_unique(out, forbidden.to_string());
            }
        }
        UseTree::Glob(_) => {
            // The parent segment was inspected by the outer
            // `Path` walker; nothing to do here.
        }
        UseTree::Group(g) => {
            for item in &g.items {
                collect_path_segments(item, forbidden, out);
            }
        }
    }
}

fn push_unique(out: &mut Vec<String>, v: String) {
    if !out.iter().any(|existing| existing == &v) {
        out.push(v);
    }
}

fn violations_in(layer_dirs: &[&Path], forbidden_layers: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for layer_dir in layer_dirs {
        for path in collect_rs_files(layer_dir) {
            let content = read(&path);
            for forbidden in forbidden_layers {
                for v in find_uses(&content, forbidden) {
                    out.push(format!("{}: {v}", path.display()));
                }
            }
        }
    }
    out
}

#[test]
fn core_has_no_dependency_on_capability_or_personality() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"));
    let core_dir = workspace.join("src/core");
    assert!(core_dir.is_dir(), "src/core should exist");

    let forbidden = vec!["capability", "personality"];
    let all_violations = violations_in(&[&core_dir], &forbidden);
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

    let violations = violations_in(&[&cap_dir], &["personality"]);
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

    let uses_capability = violations_in(&[&personality_dir], &["capability"]);
    assert!(
        !uses_capability.is_empty(),
        "src/personality/ must depend on crate::capability (cspace, \
         factory, budget). Found no such imports — the \
         orchestrator has lost its connection to the kernel?"
    );
}
