---
date: 2026-09-16T22:52:14+0800
author: anreonyr
commit: 3c8b0dc
branch: master
repository: odyssey
topic: "Manifest Field Subtraction"
tags: [design, manifest, plugin, subtraction, refactor]
status: ready
parent: ".rpiv/artifacts/research/2026-09-16_22-07-52_manifest-field-subtraction.md"
last_updated: 2026-09-16T22:52:14+0800
last_updated_by: anreonyr
---

# Design: Manifest Field Subtraction

## Summary

A pure deletion refactor across 21 files in 5 layers (kernel, personality, example backend, frontend, top-level docs). The change removes every dead or dead-via-builder field on `PluginManifest` and its nested types, deletes the supporting types (`IsolationMode`, `HostServiceRef`, `ResourceHints`), and cascades the deletion through every reader (mint, HTTP bridge, orchestrator, 2 example-backend JSON wire contracts, 2 frontend TS interfaces, 1 React JSX render site, 2 test struct literals, 1 README example, 1 CHANGELOG entry). Single commit on `phase12/chore/scrub-…-host-service-vocabulary` following Phase 9 `a42573f` precedent; all four follow-up classes (orphan Cargo deps, CHANGELOG backfill, doc-lies, trait/forward-glue) addressed in the same commit per research composite lessons. No new abstractions, no moved code, no API additions. `cargo build -p odyssey && cargo test -p odyssey && cargo test -p back` is the verification gate.

## Requirements

From the FRD at `.rpiv/artifacts/discover/2026-09-16_21-27-12_manifest-field-subtraction.md` (19 functional requirements, summarized):

1. `PluginManifest` loses `isolate: IsolationMode`, `host: Vec<HostServiceRef>`, `resources: ResourceHints`; gains `timeout_ms: Option<u32>` directly (FR1-FR3).
2. `IsolationMode`, `HostServiceRef`, `ResourceHints` types deleted (FR4-FR6).
3. `CapabilityDecl` loses `in_type: String`, `out_type: String` (FR7).
4. `CapabilityMeta` loses `in_type: String`, `out_type: String` (FR8).
5. 11 `.host("dispatcher")` call sites in `example/back/src/` drop the `.host()` chain (FR9).
6. `ManifestBuilder::build()` constructs `{ plugin, exposes, requires, timeout_ms }` and nothing else (FR10).
7. 4 `expose*` builder methods lose `in_type` / `out_type` struct-literal fields (FR11).
8. `mint.rs::meta_from_decl` loses `in_type` / `out_type` clones (FR12).
9. HTTP bridge `CapInfo` loses `in_type` / `out_type` (FR13).
10. `profile_inspector.rs::inspect` loses `in_type` / `out_type` JSON keys (FR14).
11. `agent.rs::describe` loses `in_type` / `out_type` JSON keys (FR15).
12. `CapDetail.tsx` loses 2 `<Meta>` JSX rows (FR16).
13. `ManifestBuilder`'s `host: Vec<HostServiceRef>` field removed (FR17).
14. `manifest.rs` module-level doc updated to drop `host services` clause (FR18).
15. All other doc-comments referencing removed fields scrubbed (FR19).

## Current State Analysis

### Key Discoveries

- **`PluginManifest.host` is write-only** (`manifest.rs:55`): 11 `.host("dispatcher")` call sites across `example/back/src/`, zero readers anywhere in `crate/odyssey/src` or `example/back/src` (research doc Layer 3).
- **`PluginManifest.isolate: IsolationMode` is dead-via-builder** (`manifest.rs:43`): `build()` hardcodes `IsolationMode::InProc` at `manifest.rs:362` — the field carried no information; the single variant `InProc` was the constant output of the only setter (the constructor at `manifest.rs:362`).
- **`ResourceHints` wraps a single field** (`manifest.rs:155-167`): `timeout_ms: Option<u32>` is the only field, with one reader at `personality/lifecycle/run.rs:230` (`entry.manifest.resources.timeout_ms.unwrap_or(5000)`).
- **`CapabilityDecl.in_type` / `out_type` are dead-via-builder** (`manifest.rs:112, 117`): the four `expose*` builder methods hardcode `in_type: "any".into()` / `out_type: "any".into()` at `manifest.rs:250-251, 271-272, 292-293, 314-315`. Values propagate through `CapabilityMeta` (`meta.rs:48-49`) and surface in three JSON wire contracts (`CapInfo`, `AgentCore::describe`, `ProfileInspectorResource::inspect`), but the value is always the literal string `"any"`.
- **`core/mod.rs:14-19, 36-40` is the single Rust site outside `manifest.rs` that names the three deleted types**: the `pub use manifest::manifest::{...}` re-export block at `:36-40` collapses from 7 tokens to 4 after deletion; the module doc at `:14-19` drops 3 names from the submodule list.
- **Two `CapabilityDecl` struct literals in tests must be updated** (`example/back/tests/smoke.rs:350-357, 501-508`): these literals directly construct the post-deletion shape, and `cargo test -p back` will fail with `E0560: field 'in_type' does not exist` until they are.
- **Two TypeScript interfaces in `types.tsx` mirror the Rust `CapInfo` and `agent_describe` JSON contracts** (`types.tsx:15-28, 36-51`): the hand-written TS types are the missing-fields regression net; a Rust-side field removal without a TS-side update triggers `tsc` errors that block `pnpm build`.
- **`docs/deferred/` is truly dormant**: the WASM/cdylib design sketches and `wasm-loader.wat` are not embedded, imported, or referenced by any compiled code. Per Decision 7, the deferred directory is out of scope for this change.

### Constraints

- The 1:1 builder↔data-shape invariant must hold after subtraction: every `PluginManifest` field settable via the builder, every builder method sets a field that exists on the resulting struct.
- The `..Default::default()` filler pattern must stay out of `build()` (Phase 9 CI `0b91cb9` flagged this as clippy `needless_update`).
- The `docs/deferred/` TOML sketches and `wasm-loader.wat` are documentation of a future shape, not compiled code — they must NOT be scrubbed.
- `CHANGELOG.md` historical entries (lines 210, 626, 649, 864, 867, 869) reference the deleted shapes; they are phase-history and must NOT be edited. A new `### Changed — Phase 12: …` entry is mandatory in the same commit.

## Scope

### Building

- Delete `PluginManifest.host`, `PluginManifest.isolate`, `PluginManifest.resources` (replaced by `timeout_ms: Option<u32>` directly).
- Delete `IsolationMode`, `HostServiceRef`, `ResourceHints` types.
- Delete `CapabilityDecl.in_type`, `CapabilityDecl.out_type`.
- Delete `CapabilityMeta.in_type`, `CapabilityMeta.out_type`.
- Delete `ManifestBuilder::host()` setter and `host: Vec<HostServiceRef>` internal field.
- Delete 4 `in_type` / `out_type` literals in 4 `expose*` builder methods.
- Delete 11 `.host("dispatcher")` chain calls in 10 `example/back/src/*.rs` files (`agent.rs` carries 2 of the 11).
- Delete 2 `in_type` / `out_type` JSON keys in 3 example-backend JSON wire contracts.
- Delete 2 `in_type` / `out_type` fields from 2 TypeScript interfaces in `types.tsx`.
- Delete 2 `<Meta>` JSX rows in `CapDetail.tsx`.
- Delete 2 `in_type` / `out_type` lines from 2 test struct literals in `smoke.rs`.
- Update `core/mod.rs` re-export block + module doc.
- Update `mint.rs::meta_from_decl`, `serve.rs::CapInfo`, `run.rs:230` (one-token rename).
- Scrub 18 doc-comment blocks inside `manifest.rs` per Decision 7.
- Add `### Changed — Phase 12: manifest field subtraction cascade` entry to `CHANGELOG.md` under `[Unreleased]`.
- Update `README.md:356` JSON example (drop `"in_type":"any","out_type":"any",`).
- Single commit with `phase12/chore/scrub-…-host-service-vocabulary` title.

### Not Building

- **`requires.kind` / `optional` / `VersionRange`** — needs a resolver consumer that enforces type-compatible bindings; deferred per research Composite Lesson 6.
- **`IsolationMode` Wasm/Subprocess variants** — deferred until WASM/cdylib loaders ship (per the `manifest.rs:22-26` doc-comment, preserved).
- **`Permissions` block** (`can_spawn_subprocess`, `can_read_fs`, `can_open_network`) — deferred until a host sandbox is in place to consume it.
- **Strongly-typed `tool_schema`** (`ToolSchema` enum with `OpenAi` / `JsonSchema` variants) — deferred until a second consumer beyond `tool_descriptor` reads it.
- **`docs/deferred/` changes** — out of scope per Decision 7; the WASM/cdylib TOML sketches and `wasm-loader.wat` are documentation of intent only.
- **Any semantic change to capability resolution, minting, or runtime** — out of scope; this is a pure deletion refactor.
- **Any change to `PluginId` or `CapKind`** — kept as-is.

## Decisions

### Decision 1 — Single-slice decomposition (1 commit, 21 files)

**Question**: How to decompose the 21-file cascade into slices for review?
**Chosen**: 1 slice. The 21 files are mechanically interdependent (kernel types → kernel readers → backend readers → frontend readers → tests → docs); splitting into 2+ slices would create "code done, doc pending" intermediate states that violate the `1:1 builder↔data-shape` invariant.
**Rationale**: User stated preference `先做减法` plus Phase 9 `a42573f` precedent (single 15-file commit, 70 insertions / 440 deletions). The skill's 512-1024 token per-slice target is exceeded (this slice is ~2500 tokens of diff), but a single review unit is the user's stated preference. Evidence: `a42573f` `git show --stat` showed 15 files in one commit.

### Decision 2 — Doc-comment scrub strategy: strict Decision 7 (delete-with-field, no new prose)

**Question**: How should doc-comments be updated to reflect the new shape?
**Chosen**: Strict per the user's discover Decision 7. Delete the doc-comment along with the field; do NOT add a `/// Deleted in Phase 12` placeholder; do NOT add a trailing paragraph to the `PluginManifest` struct doc or the module doc explaining the deletions.
**Rationale**: User stated `只清理死的` in discover. This overrides Phase 9's pattern (`a42573f` added 7-line + 9-line paragraphs to the `ManifestBuilder` struct doc and module doc explaining the deleted setters). For this change, the 18 doc-comment scrub blocks in `manifest.rs` are pure deletions; no new prose is added.

### Decision 3 — Verifier gate: `cargo build -p odyssey && cargo test -p odyssey && cargo test -p back`

**Question**: What commands must pass after the subtraction to consider the change done?
**Chosen**: `cargo build -p odyssey && cargo test -p odyssey && cargo test -p back` (FRD Decision 6). No `cargo clippy -- -D warnings` added despite the precedent at `0b91cb9` (Phase 9 CI); user said `全量 build+test` only in discover.
**Rationale**: The `-p back` test run is the meaningful gate because it exercises manifest construction paths via the example plugins. `cargo clippy` is not in scope.

### Decision 4 — Verifier rg-gates (post-merge)

**Question**: What grep-based negative checks confirm no stragglers survive?
**Chosen**: After merge, all of the following must return zero matches:

- `rg '\.host\(' crate/odyssey/src` — no kernel calls of `.host()` (0 expected; the 11 call sites are all in `example/back/src/`)
- `rg 'in_type|out_type' crate/odyssey/src example/back/src` — no remaining readers in Rust code (0 expected; the TS interfaces are in `example/fore/src/`)
- `rg 'IsolationMode|HostServiceRef|ResourceHints' crate/odyssey/src` — 0 matches outside `meta.rs::` (which keeps `QuotaKind` etc. but loses the manifest-related types)
- `rg 'manifest\.resources' crate/odyssey/src` — 0 matches (the only reader was `run.rs:230` which renames to `manifest.timeout_ms`)
**Rationale**: Research Composite Lesson 5 — "The verifier gate after a compound deletion is `rg`-based, not compiler-based." A `cargo build` clean exit does not catch orphan type references that pass lint as `dead_code` warnings (the silent-pass failure mode surfaced in the research doc's "build() body list-every-field-explicit" analysis).

### Decision 5 — CHANGELOG entry: `### Changed — Phase 12: …` at top of `[Unreleased]`

**Question**: Where and how to add the CHANGELOG entry?
**Chosen**: Insert at the top of `CHANGELOG.md`'s `[Unreleased]` section (immediately under the `## [Unreleased]` heading at `:7`, before the `### Added — oxlint + oxfmt…` entry at `:9`). Heading: `### Changed — Phase 12: manifest field subtraction cascade`. Style: first-person plural, bold bullet leads, double-backticked code identifiers, no `file:line` references in bullets (per Keep-a-Changelog convention; prior entries do not cite line numbers).
**Rationale**: Phase 9 + 9.5 + 10 entry at `CHANGELOG.md:376` is the closest precedent (multi-area cleanup pass, 167 lines, `### Changed — …` heading). Phase 12 is the next integer (Phase 11 at `CHANGELOG.md:339` is the most recent; no Phase 11.5 in git history). 16 bullets, one per logical deletion site (one per file group). Length target: 60-90 lines.

### Decision 6 — Commit title: `phase12/chore/scrub-manifest-dead-fields-and-host-service-vocabulary`

**Question**: What is the commit title?
**Chosen**: `phase12/chore/scrub-manifest-dead-fields-and-host-service-vocabulary`. Category: `chore` (matches `a42573f`'s `phase9/chore/scrub-…-dead-code`). Verb: `scrub-…-dead-…` (project idiom for "delete cluster of dead symbols identified by audit").
**Rationale**: Pattern from `a42573f` (Phase 9 scrub). The title is 56 chars (under 72).

### Decision 7 — Commit body: per-file enumeration matching `a42573f` structure

**Question**: How to structure the commit body?
**Chosen**: 5 sections (matching `a42573f`):

1. Opening rationale (1-2 sentences naming the audit source and scope).
2. Per-file enumeration with `Deleted from <path>:` headers and one-line justifications.
3. Cross-cutting concerns section (`CHANGELOG.md updated:`, `README.md updated:`).
4. Builtin consumer section (`11 .host("dispatcher")` call sites listed in one section).
5. No `Signed-off-by`, no PR footer, no `Co-authored-by`.

**Rationale**: `a42573f` body is the document of record for compound-deletion commits. Per Research Composite Lesson 1: "delete the writer, the reader, and the warnings in one commit."

## Architecture

### `crate/odyssey/src/core/manifest/manifest.rs` — MODIFY

**Purpose**: Target file. Drops 3 fields from `PluginManifest` (`isolate`, `host`, `resources`), drops 1 enum (`IsolationMode`), drops 2 structs (`HostServiceRef`, `ResourceHints`), drops 1 builder method (`host()`), drops 4 `in_type`/`out_type` literals from 4 `expose*` methods, scrubs 18 doc-comment blocks.

```rust
//! Plugin manifest — data shape + builder.
//!
//! Phase 8: this file is the merge of the Phase 5 split
//! (`types.rs` + `builder.rs` + `load.rs` + `mod.rs`) into one
//! cohesive module. The split was justified while personality
//! owned the manifest types; in the new layout the manifest is
//! a leaf shared value type, and one file is enough.
//!
//! Phase 9 cleanup: the TOML loader (`from_toml_str`,
//! `from_path`) and the `validate` method are deleted. The
//! Phase 5/6/7 era of plugins loading manifests from disk is
//! over; every runtime plugin (and now every builtin) builds
//! its manifest in code via `ManifestBuilder`. The `toml` and
//! `serde::Deserialize` derives stay so a future TOML file
//! can re-attach if the deferred WASM / cdylib loaders come
//! back, but the wiring helpers are gone.
//!
//! Manifests describe plugin identity, the exposed capability
//! surface, and dependencies on other plugins' capabilities.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use crate::core::identity::ids::PluginId;
use crate::core::identity::kind::CapKind;

// ---------------------------------------------------------------------------
// Data shapes
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginManifest {
    pub plugin: PluginId,
    #[serde(default)]
    pub exposes: Vec<CapabilityDecl>,
    /// Phase 3 P3.1 — capability-keyed dependencies. The resolver
    /// matches each `requires[*].contract` against another
    /// manifest's `[[exposes]] contract_name`. A plugin declares
    /// what contract it needs, not which plugin provides it; the
    /// resolver decides. Empty `requires` means the plugin is a
    /// root provider (or has no capability dependencies).
    #[serde(default)]
    pub requires: Vec<CapabilityRequirement>,
    /// Per-call wall-clock budget in milliseconds. Default `None`
    /// lets the orchestrator's `CapabilityBudget::new(5000)` use
    /// the host default.
    #[serde(default)]
    pub timeout_ms: Option<u32>,
}

/// Phase 3 P3.1 — a capability-keyed dependency.
///
/// A plugin's runtime behaviour is determined by which contracts
/// it can bind to. The resolver at boot time scans every loaded
/// manifest, builds a `contract_name → provider plugin` index,
/// then walks each plugin's `requires` list to determine mint
/// order and bindings.
///
/// Example:
/// ```toml
/// [[requires]]
/// name     = "embedder"      # handle inside this plugin
/// contract = "embedder"      # the contract to bind against
/// ```
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilityRequirement {
    /// Local handle name. The plugin code uses this string to
    /// look up the received slot reference. By convention,
    /// matches the provider's capability name but isn't required
    /// to — the handle is purely local.
    pub name: String,
    /// Contract name to bind against. Must match exactly one
    /// `[[exposes]] contract_name` from another loaded manifest,
    /// or boot fails with `ResolveError::Unprovided` (no
    /// provider) or `ResolveError::Ambiguous` (multiple providers
    /// without a priority hint).
    pub contract: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilityDecl {
    pub name: String,
    /// Runtime kind (sync vs stream). Phase 9 cleanup: replaced
    /// the previous `streaming: bool` field. The bool could only
    /// express two states; the `CapKind` enum is the single
    /// source of truth for kind across the kernel, the runtime
    /// meta, and the manifest. Serde's `default` keeps the
    /// manifest TOML shape honest (a missing `kind` parses as
    /// `Sync`); the legacy `streaming: bool` field is no longer
    /// recognised.
    #[serde(default)]
    pub kind: CapKind,
    /// Phase 3 P3.1 — the contract name this capability publishes.
    /// Other plugins declare a matching `contract` in their
    /// `[[requires]]` block to be bound to this capability at boot.
    /// Empty string means "no contract published" — such caps are
    /// only reachable by direct slot lookup, not by capability
    /// injection. Conventional form is lowercase dotted, e.g.
    /// `"generator"`, `"embedder"`, `"database"`, `"agent"`.
    #[serde(default)]
    pub contract_name: String,
    /// Optional tool schema — a JSON object the agent reads via the
    /// `tool_descriptor` capability to learn the tool's input/output
    /// contract. `None` means "no schema published" — the cap is not
    /// agent-callable in a schema-driven way (an agent can still call
    /// it by name through the cspace, just without structured schema
    /// metadata). The value is opaque to the kernel; it's the
    /// `tool_descriptor` plugin that interprets it.
    #[serde(default)]
    pub tool_schema: Option<Value>,
}

// ---------------------------------------------------------------------------
// Fluent builder
// ---------------------------------------------------------------------------

/// Builder for [`PluginManifest`] — Phase 3 manifest template.
///
/// Replaces the toml parser + the 50-line struct-literal each
/// plugin used to write. Each runtime plugin's `manifest.rs`
/// becomes a few lines of fluent builder calls. Defaults fill in
/// the convention:
///
/// | Field          | Default               |
/// |----------------|-----------------------|
/// | `version`      | `"0.1.0"`             |
/// | `kind`         | `CapKind::Sync`       |
/// | `exposes`      | `[]`                  |
/// | `requires`     | `[]`                  |
/// | `timeout_ms`   | `None` → host default |
///
/// Phase 9: the previous `.action(name, op)` and `.protocol(p)`
/// setters are gone — they fed the now-deleted
/// `AuthorityContract` / `Protocol` fields that were
/// write-only metadata. If a future cap needs to advertise a
/// typed contract vocabulary, add it back at the same time the
/// reader is added.
pub struct ManifestBuilder {
    name: String,
    version: String,
    exposes: Vec<CapabilityDecl>,
    requires: Vec<CapabilityRequirement>,
    timeout_ms: Option<u32>,
}

impl ManifestBuilder {
    /// Start a manifest for a plugin named `plugin_name`. The
    /// plugin's capabilities are appended with [`Self::expose`] /
    /// [`Self::expose_streaming`]; a manifest with none is a legal
    /// but useless plugin, so `build` does not reject it.
    pub fn new(plugin_name: impl Into<String>) -> Self {
        Self {
            name: plugin_name.into(),
            version: "0.1.0".into(),
            exposes: Vec::new(),
            requires: Vec::new(),
            timeout_ms: None,
        }
    }

    /// Override the plugin version (default `"0.1.0"`).
    pub fn version(mut self, v: impl Into<String>) -> Self {
        self.version = v.into();
        self
    }

    /// Append a sync capability. `contract_name` is what another
    /// plugin's `requires` matches against, so it is required
    /// here even though `CapabilityDecl::contract_name` allows an
    /// empty string for caps that publish nothing.
    pub fn expose(mut self, name: impl Into<String>, contract_name: impl Into<String>) -> Self {
        self.exposes.push(CapabilityDecl {
            name: name.into(),
            kind: CapKind::Sync,
            contract_name: contract_name.into(),
            tool_schema: None,
        });
        self
    }

    /// Append a sync capability and attach a `tool_schema` JSON
    /// object. Use this when the capability is meant to be called by
    /// the agent through `tool_descriptor`; the schema is opaque to
    /// the kernel.
    pub fn expose_with_schema(
        mut self,
        name: impl Into<String>,
        contract_name: impl Into<String>,
        tool_schema: Value,
    ) -> Self {
        self.exposes.push(CapabilityDecl {
            name: name.into(),
            kind: CapKind::Sync,
            contract_name: contract_name.into(),
            tool_schema: Some(tool_schema),
        });
        self
    }

    /// Append a streaming capability. Separate from [`Self::expose`]
    /// because the kind is the whole difference and `CapKind` has
    /// exactly two variants: a `kind` parameter would let a caller
    /// pass `Sync` to a method named `expose` and read as though
    /// they had asked for a stream.
    pub fn expose_streaming(
        mut self,
        name: impl Into<String>,
        contract_name: impl Into<String>,
    ) -> Self {
        self.exposes.push(CapabilityDecl {
            name: name.into(),
            kind: CapKind::Stream,
            contract_name: contract_name.into(),
            tool_schema: None,
        });
        self
    }

    /// Append a streaming capability with a `tool_schema`.
    /// Symmetric with [`Self::expose_with_schema`]; the
    /// schema's `output_schema` describes one chunk of the
    /// stream, and the cap's `kind` tells the LLM the result
    /// is a stream of those.
    pub fn expose_streaming_with_schema(
        mut self,
        name: impl Into<String>,
        contract_name: impl Into<String>,
        tool_schema: Value,
    ) -> Self {
        self.exposes.push(CapabilityDecl {
            name: name.into(),
            kind: CapKind::Stream,
            contract_name: contract_name.into(),
            tool_schema: Some(tool_schema),
        });
        self
    }

    /// Append a `[[requires]]` entry. May be called multiple
    /// times to declare multiple capability-keyed dependencies.
    pub fn requires(mut self, handle: impl Into<String>, contract: impl Into<String>) -> Self {
        self.requires.push(CapabilityRequirement {
            name: handle.into(),
            contract: contract.into(),
        });
        self
    }

    /// Set the timeout hint in milliseconds (default `None`,
    /// which lets `CapabilityBudget::new` use the host default).
    pub fn timeout_ms(mut self, ms: u32) -> Self {
        self.timeout_ms = Some(ms);
        self
    }

    /// Materialise the manifest. Consumes the builder.
    pub fn build(self) -> PluginManifest {
        let ManifestBuilder {
            name,
            version,
            exposes,
            requires,
            timeout_ms,
        } = self;
        PluginManifest {
            plugin: PluginId { name, version },
            exposes,
            requires,
            timeout_ms,
        }
    }
}
```

### `crate/odyssey/src/core/mod.rs` — MODIFY

**Purpose**: Drops 3 tokens (`HostServiceRef`, `IsolationMode`, `ResourceHints`) from `pub use manifest::manifest::{...}` re-export block at `:36-40`; drops 3 names from module doc at `:14-19`.

```rust
//! - `manifest` — `PluginManifest`, `ManifestBuilder`,
//!   `CapabilityDecl`, `CapabilityRequirement`.

// Curated re-exports — the public surface of core.
pub use manifest::manifest::{
    CapabilityDecl, CapabilityRequirement, ManifestBuilder, PluginManifest,
};
```

### `crate/odyssey/src/core/meta/meta.rs` — MODIFY

**Purpose**: Drops 2 fields (`in_type`, `out_type`) from `CapabilityMeta` struct (lines 48-49).

```rust
pub struct CapabilityMeta {
    pub id: CapabilityId,
    pub name: String,
    pub namespace: String,
    pub contract_name: String,
    pub plugin: PluginId,
    pub kind: CapKind,
    pub timeout_ms: u32,
    pub quota: QuotaSpec,
    pub tool_schema: Option<Value>,
}
```

### `crate/odyssey/src/personality/lifecycle/mint.rs` — MODIFY

**Purpose**: Drops 2 clone writes (`in_type: decl.in_type.clone()`, `out_type: decl.out_type.clone()`) in `meta_from_decl` at lines 56-57.

```rust
pub fn meta_from_decl(
    id: CapabilityId,
    decl: &CapabilityDecl,
    plugin: &PluginId,
    budget: &CapabilityBudget,
) -> CapabilityMeta {
    CapabilityMeta {
        id,
        name: decl.name.clone(),
        namespace: namespace_for(plugin, &decl.name),
        contract_name: decl.contract_name.clone(),
        plugin: plugin.clone(),
        kind: decl.kind,
        timeout_ms: budget.timeout_ms(),
        quota: budget.quota_spec(),
        tool_schema: decl.tool_schema.clone(),
    }
}
```

### `crate/odyssey/src/personality/lifecycle/serve.rs` — MODIFY

**Purpose**: Drops 2 fields (`in_type: String`, `out_type: String`) from `CapInfo` struct at lines 75-76; drops 2 clone writes in `list_caps` at lines 195-196.

```rust
#[derive(Serialize)]
struct CapInfo {
    name: String,
    id: String,
    /// Serialised as the legacy `streaming: bool` field for
    /// HTTP API back-compat. The runtime meta now carries
    /// `kind: CapKind`; the bridge flattens it to a bool for the
    /// JSON payload. (A future `?as_kind=` extension can
    /// expose the full enum; today no client needs it.)
    streaming: bool,
    timeout_ms: u32,
}

async fn list_caps(State(state): State<AppState>) -> Json<Vec<CapInfo>> {
    Json(
        state
            .cspace
            .enumerate()
            .into_iter()
            .map(|m: CapabilityMeta| CapInfo {
                name: m.name.clone(),
                id: m.id.to_string(),
                streaming: m.kind == CapKind::Stream,
                timeout_ms: m.timeout_ms,
            })
            .collect(),
    )
}
```

### `crate/odyssey/src/personality/lifecycle/run.rs` — MODIFY

**Purpose**: One-token rename at line 230 — `entry.manifest.resources.timeout_ms.unwrap_or(5000)` → `entry.manifest.timeout_ms.unwrap_or(5000)`.

```rust
        for decl in &entry.manifest.exposes {
            let budget = CapabilityBudget::new(entry.manifest.timeout_ms.unwrap_or(5000));
            let slot_id = (entry.mint_fn)(factory, plugin_id, decl, decl.kind, budget, bindings);
```

### `example/back/src/echo.rs` — MODIFY

**Purpose**: Drops `.host("dispatcher")` chain call at line 58 in `EchoBuiltin::manifest()`.

```rust
        ManifestBuilder::new("echo")
            .expose_with_schema("echo", "echo", tool_schema)
            .timeout_ms(5000)
            .build()
```

### `example/back/src/reverse.rs` — MODIFY

**Purpose**: Drops `.host("dispatcher")` chain call at line 71 in `ReverseBuiltin::manifest()`.

```rust
        ManifestBuilder::new("reverse")
            .expose_with_schema("reverse", "reverse", tool_schema)
            .timeout_ms(5000)
            .build()
```

### `example/back/src/database.rs` — MODIFY

**Purpose**: Drops `.host("dispatcher")` chain call at line 165 in `DatabaseBuiltin::manifest()`.

```rust
        ManifestBuilder::new("database")
            .expose_with_schema("database", "database", tool_schema)
            .timeout_ms(5000)
            .build()
```

### `example/back/src/streaming_echo.rs` — MODIFY

**Purpose**: Drops `.host("dispatcher")` chain call at line 144 in `StreamingEchoBuiltin::manifest()`.

```rust
        ManifestBuilder::new("streaming_echo")
            .expose_streaming_with_schema("streaming_echo", "streaming_echo", tool_schema)
            .timeout_ms(5000)
            .build()
```

### `example/back/src/llm.rs` — MODIFY

**Purpose**: Drops `.host("dispatcher")` chain call at line 1095 in `LlmBuiltin::manifest()`.

```rust
        ManifestBuilder::new("llm")
            .expose(NAME_COMPLETE, CONTRACT_COMPLETE)
            .expose(NAME_EMBED, CONTRACT_EMBED)
            .timeout_ms(30000)
            .build()
```

### `example/back/src/memory.rs` — MODIFY

**Purpose**: Drops `.host("dispatcher")` chain call at line 506 in `MemoryBuiltin::manifest()`.

```rust
        ManifestBuilder::new("memory")
            .expose(NAME_QUERY, CONTRACT_QUERY)
            .expose(NAME_INSERT, CONTRACT_INSERT)
            .timeout_ms(5000)
            .build()
```

### `example/back/src/tool_descriptor.rs` — MODIFY

**Purpose**: Drops `.host("dispatcher")` chain call at line 169 in `ToolDescriptorBuiltin::manifest()`.

```rust
        ManifestBuilder::new("tool_descriptor")
            .expose(NAME, CONTRACT)
            .timeout_ms(5000)
            .build()
```

### `example/back/src/profile_inspector.rs` — MODIFY

**Purpose**: Drops `.host("dispatcher")` chain call at line 149 in `ProfileInspectorBuiltin::manifest()`; drops 2 JSON keys (`"in_type": meta.in_type`, `"out_type": meta.out_type`) in `ProfileInspectorResource::inspect` at lines 95-96.

```rust
        ManifestBuilder::new("profile_inspector")
            .expose(NAME, CONTRACT)
            .timeout_ms(5000)
            .build()

// In inspect():
        Ok(json!({
            "subject": subject,
            "meta": {
                "id": meta.id.to_string(),
                "name": meta.name,
                "namespace": meta.namespace,
                "contract_name": meta.contract_name,
                "plugin": { "name": meta.plugin.name, "version": meta.plugin.version },
                "kind": kind_name,
                "timeout_ms": meta.timeout_ms,
                "quota": {
                    "calls_per_minute": meta.quota.calls_per_minute,
                },
                "has_tool_schema": meta.tool_schema.is_some(),
            },
            "operations": operation_names(cap.operations()),
        }))
```

### `example/back/src/agent_runtime.rs` — MODIFY

**Purpose**: Drops `.host("dispatcher")` chain call at line 1548 in `AgentRuntimeBuiltin::manifest()`.

```rust
        ManifestBuilder::new("agent_runtime")
            .expose(NAME_START, CONTRACT_START)
            .expose(NAME_RESUME, CONTRACT_RESUME)
            .expose(NAME_CANCEL, CONTRACT_CANCEL)
            .expose(NAME_PLAN, CONTRACT_PLAN)
            .expose_streaming(NAME_STREAM, CONTRACT_STREAM)
            .expose(NAME_RECALL, CONTRACT_RECALL)
            .expose(NAME_RECORD, CONTRACT_RECORD)
            .expose(NAME_LOAD, CONTRACT_LOAD)
            .requires(REACHES[0].0, REACHES[0].1)
            .requires(REACHES[1].0, REACHES[1].1)
            .requires(REACHES[2].0, REACHES[2].1)
            .requires(REACHES[3].0, REACHES[3].1)
            .timeout_ms(30000)
            .build()
```

### `example/back/src/agent.rs` — MODIFY

**Purpose**: Drops 2 `.host("dispatcher")` chain calls at lines 295 and 340 in `AgentListBuiltin::manifest()` and `AgentDescribeBuiltin::manifest()`; drops 2 JSON keys (`"in_type": meta.in_type`, `"out_type": meta.out_type`) in `AgentCore::describe` at lines 185-186.

```rust
        ManifestBuilder::new("agent_list")
            .expose(CONTRACT_LIST, CONTRACT_LIST)
            .requires("echo", "echo")
            .requires("reverse", "reverse")
            .requires("database", "database")
            .requires("streaming_echo", "streaming_echo")
            .timeout_ms(5000)
            .build()

// And:
        ManifestBuilder::new("agent_describe")
            .expose(CONTRACT_DESCRIBE, CONTRACT_DESCRIBE)
            .requires("echo", "echo")
            .requires("reverse", "reverse")
            .requires("database", "database")
            .requires("streaming_echo", "streaming_echo")
            .timeout_ms(5000)
            .build()

// In AgentCore::describe():
        Ok(json!({
            "handle": binding.handle,
            "live": true,
            "capability": binding.capability,
            "contract": binding.contract,
            "name": meta.name,
            "namespace": meta.namespace,
            "plugin": meta.plugin.name.as_str(),
            "kind": kind_name(meta.kind),
            "streaming": meta.kind == CapKind::Stream,
            "timeout_ms": meta.timeout_ms,
            "calls_per_minute": meta.quota.calls_per_minute,
            "operations": operation_names(cap.operations()),
        }))
```

### `example/back/tests/smoke.rs` — MODIFY

**Purpose**: Drops 2 lines (`in_type: "any".into()`, `out_type: "any".into()`) from 2 `CapabilityDecl` struct literals (lines 350-357 in `agent_reaches_only_its_bindings_and_reports_revocation`, lines 501-508 in `agent_describe_refuses_unknown_fields`).

```rust
    let plain_decl = CapabilityDecl {
        name: "plain".into(),
        kind: CapKind::Sync,
        contract_name: "plain".into(),
        tool_schema: None,
    };

    // (and at lines 501-508)
    let decl = CapabilityDecl {
        name: "echo".into(),
        kind: CapKind::Sync,
        contract_name: "echo".into(),
        tool_schema: None,
    };
```

### `example/fore/src/api/types.tsx` — MODIFY

**Purpose**: Drops `in_type: string` / `out_type: string` from `CapInfo` interface (lines 24-25) and from `AgentHandleDescribeLive` interface (lines 47-48).

```typescript
export interface CapInfo {
  /** Capability name registered in the cspace. */
  name: string;
  /** Stable capability id. */
  id: string;
  /** True if this cap's `kind == CapKind::Stream`. */
  streaming: boolean;
  /** Per-call wall-clock budget, ms. */
  timeout_ms: number;
}

export interface AgentHandleDescribeLive {
  handle: string;
  live: true;
  capability: string;
  contract: string;
  name: string;
  namespace: string;
  plugin: string;
  kind: "sync" | "stream";
  streaming: boolean;
  timeout_ms: number;
  calls_per_minute: number;
  operations: Array<"READ" | "WRITE" | "EXECUTE" | "ADMIN">;
}
```

### `example/fore/src/pages/CapDetail.tsx` — MODIFY

**Purpose**: Drops 2 `<Meta>` JSX rows (`<Meta label="in" value={cap.in_type} mono />`, `<Meta label="out" value={cap.out_type} mono />`) at lines 113-114.

```tsx
              <Meta label="id" value={cap.id} mono />
              <Meta label="plugin" value={capToPlugin(cap.name)} mono />
              <Meta label="timeout" value={`${cap.timeout_ms} ms`} mono />
              <Separator />
```

### `CHANGELOG.md` — MODIFY

**Purpose**: Add new `### Changed — Phase 12: manifest field subtraction cascade` entry at top of `[Unreleased]` section, before the existing `### Added — oxlint + oxfmt…` entry at line 9.

```markdown
### Changed — Phase 12: manifest field subtraction cascade

A single-commit cleanup that prunes write-only and
dead-via-builder fields from `PluginManifest`, deletes
their supporting types, and cascades the deletion through
every reader. The shape of `PluginManifest` collapses from
6 fields to 4; the manifest builder loses one setter;
three supporting types are gone entirely; `CapabilityDecl`
loses its `in_type` / `out_type` fields; the same two
fields disappear from `CapabilityMeta`. The `/api/caps`
and `agent_describe` JSON wire contracts drop their
`in_type` / `out_type` keys to match. 21 files modified
across 5 layers, plus 18 doc-comment blocks inside
`manifest.rs` scrubbed of dead references.

- **`PluginManifest.host` / `HostServiceRef` deleted.**
  11 `.host("dispatcher")` call sites in `example/back/src/`
  (across 9 builtin files; `agent.rs` carries two of
  the eleven) drop the trailing `.host("dispatcher")` call.
  Zero readers anywhere in `crate/odyssey/src` or
  `example/back/src`.
- **`PluginManifest.isolate` / `IsolationMode` deleted.**
  The builder hardcoded `IsolationMode::InProc`. The Wasm /
  Subprocess loaders under `docs/deferred/` will re-attach
  to a new enum when they ship.
- **`ResourceHints` flattened to a flat `timeout_ms:
  Option<u32>` field on `PluginManifest`.** Phase 8's
  `9723503` already shrank `ResourceHints` to its single
  field; this finishes the job. The one reader (the
  orchestrator's per-call budget lookup) renames one token.
- **`CapabilityDecl.in_type` / `out_type` deleted.**
  Dead-via-builder — every `expose*` builder method set
  them to the constant `"any"`.
- **`CapabilityMeta.in_type` / `out_type` deleted.** Mirror
  of `CapabilityDecl`.
- **`ManifestBuilder.host()` setter deleted.** The
  builder struct's `host: Vec<HostServiceRef>` field
  drops along with it.
- **`core::mod.rs` re-export block slimmed.** Drops
  `HostServiceRef`, `IsolationMode`, `ResourceHints`
  (three tokens). Stale `pub use` would fail
  `cargo build -p odyssey`.
- **HTTP bridge `CapInfo` and frontend `CapInfo` /
  `AgentHandleDescribeLive` interfaces drop
  `in_type` / `out_type`.** Hand-written TS types are
  the missing-fields regression net — without them a
  Rust-side removal would silently leave an unused
  `cap.in_type` reference in JSX.
- **`CapDetail.tsx` `<Meta>` rows for `in_type` /
  `out_type` deleted.** `pages/Invoke.tsx`, `pages/Caps.tsx`,
  `hooks/useCaps.tsx`, `api/client.tsx` import `CapInfo` but
  never read the dropped fields.
- **`profile_inspector.rs` and `agent.rs` JSON
  envelopes drop `in_type` / `out_type`.**
- **`smoke.rs` literals updated.** Two `CapabilityDecl`
  struct literals in `agent_core_describes_handles`
  and `agent_describe_refuses_unknown_fields` drop the
  `in_type` / `out_type` lines.
- **JSON example in the README updated.** The
  embedded `agent_describe` response drops
  `"in_type":"any","out_type":"any",`.
- **Doc-comment scrub: 18 blocks inside `manifest.rs`.**
  Module doc drops the `host services` and `InProc
  isolation` clauses; the `ManifestBuilder` defaults
  table drops the `in_type` / `out_type` / `host` rows;
  the historical-narrative paragraph about the
  singular setters deletes. No new prose is added
  (per "only clean up dead references").

`cargo build -p odyssey && cargo test -p odyssey &&
cargo test -p back` is the verification gate. No
Cargo dependencies become orphans.
```

### `README.md` — MODIFY

**Purpose**: Drops `"in_type":"any","out_type":"any",` from the `agent_describe` JSON example at line 356 in the `## Agent builtin` section.

```
"streaming":false,
```

## Slices

### Slice 1: Manifest Field Subtraction — single commit

**Files**: All 21 architecture entries above (`manifest.rs`, `core/mod.rs`, `meta.rs`, `mint.rs`, `serve.rs`, `run.rs`, 10 `example/back/src/*.rs` files, `smoke.rs`, `types.tsx`, `CapDetail.tsx`, `CHANGELOG.md`, `README.md`).

#### Automated Verification

- [ ] Type checking passes: `cargo build -p odyssey`
- [ ] Tests pass: `cargo test -p odyssey`
- [ ] Tests pass: `cargo test -p back`
- [ ] No `.host(` calls in kernel: `rg '\.host\(' crate/odyssey/src` returns 0 matches
- [ ] No `in_type`/`out_type` in Rust: `rg 'in_type|out_type' crate/odyssey/src example/back/src` returns 0 matches
- [ ] No `IsolationMode`/`HostServiceRef`/`ResourceHints` in kernel: `rg 'IsolationMode|HostServiceRef|ResourceHints' crate/odyssey/src` returns 0 matches
- [ ] No `manifest.resources` access: `rg 'manifest\.resources' crate/odyssey/src` returns 0 matches
- [ ] No remaining `.host(` in example backend: `rg '\.host\(' example/back/src` returns 0 matches
- [ ] Frontend types compile: `pnpm --dir example/fore build` exits 0 (or its equivalent)
- [ ] 1:1 builder↔data-shape invariant: every `PluginManifest` field is settable via the builder, every builder method sets a field that exists

#### Manual Verification

- [ ] Frontend `CapDetail` card no longer shows `in: any` / `out: any` rows
- [ ] `/api/caps` JSON response no longer contains `in_type` / `out_type` keys
- [ ] `agent_describe` JSON response no longer contains `in_type` / `out_type` keys
- [ ] `profile_inspect` JSON response no longer contains `in_type` / `out_type` keys
- [ ] `CHANGELOG.md` `[Unreleased]` section has the new `### Changed — Phase 12: …` entry
- [ ] `docs/deferred/` TOML sketches are unchanged (no scrub)
- [ ] `CHANGELOG.md` historical entries at lines 210, 626, 649, 864, 867, 869 are unchanged (no scrub)

## Desired End State

```rust
// A builtin plugin after the subtraction — simpler chain, no .host() call:
ManifestBuilder::new("echo")
    .expose_with_schema("echo", "echo", tool_schema)
    .timeout_ms(5000)
    .build()

// Resulting PluginManifest shape (4 fields, 1:1 with builder):
PluginManifest {
    plugin: PluginId { name: "echo".into(), version: "0.1.0".into() },
    exposes: vec![CapabilityDecl {
        name: "echo".into(),
        kind: CapKind::Sync,
        contract_name: "echo".into(),
        tool_schema: Some(tool_schema),
    }],
    requires: vec![],
    timeout_ms: Some(5000),
}

// The orchestrator's per-call budget site (one-token rename):
let budget = CapabilityBudget::new(entry.manifest.timeout_ms.unwrap_or(5000));

// The HTTP bridge /api/caps response (no in_type / out_type keys):
[
    {
        "name": "echo",
        "id": "cap:1",
        "streaming": false,
        "timeout_ms": 5000
    }
]

// The TypeScript CapInfo interface (no in_type / out_type):
interface CapInfo {
  name: string;
  id: string;
  streaming: boolean;
  timeout_ms: number;
}
```

## File Map

| Path | Type | Purpose |
| ------ | ------ | --------- |
| `crate/odyssey/src/core/manifest/manifest.rs` | MODIFY | Target — drops 3 fields, 1 enum, 2 structs, 1 builder method, 4 builder literals, 18 doc blocks |
| `crate/odyssey/src/core/mod.rs` | MODIFY | Re-export block + module doc collapse |
| `crate/odyssey/src/core/meta/meta.rs` | MODIFY | `CapabilityMeta` loses 2 fields |
| `crate/odyssey/src/personality/lifecycle/mint.rs` | MODIFY | `meta_from_decl` drops 2 clones |
| `crate/odyssey/src/personality/lifecycle/serve.rs` | MODIFY | `CapInfo` struct + `list_caps` drop 2 fields + 2 clones |
| `crate/odyssey/src/personality/lifecycle/run.rs` | MODIFY | One-token rename at line 230 |
| `example/back/src/echo.rs` | MODIFY | Drop `.host("dispatcher")` at line 58 |
| `example/back/src/reverse.rs` | MODIFY | Drop `.host("dispatcher")` at line 71 |
| `example/back/src/database.rs` | MODIFY | Drop `.host("dispatcher")` at line 165 |
| `example/back/src/streaming_echo.rs` | MODIFY | Drop `.host("dispatcher")` at line 144 |
| `example/back/src/llm.rs` | MODIFY | Drop `.host("dispatcher")` at line 1095 |
| `example/back/src/memory.rs` | MODIFY | Drop `.host("dispatcher")` at line 506 |
| `example/back/src/tool_descriptor.rs` | MODIFY | Drop `.host("dispatcher")` at line 169 |
| `example/back/src/profile_inspector.rs` | MODIFY | Drop `.host("dispatcher")` at line 149 + 2 JSON keys at 95-96 |
| `example/back/src/agent_runtime.rs` | MODIFY | Drop `.host("dispatcher")` at line 1548 |
| `example/back/src/agent.rs` | MODIFY | Drop 2 `.host("dispatcher")` at 295, 340 + 2 JSON keys at 185-186 |
| `example/back/tests/smoke.rs` | MODIFY | Drop 2 lines from each of 2 struct literals (350-357, 501-508) |
| `example/fore/src/api/types.tsx` | MODIFY | Drop 2 fields from 2 interfaces (24-25, 47-48) |
| `example/fore/src/pages/CapDetail.tsx` | MODIFY | Drop 2 `<Meta>` JSX rows (113-114) |
| `CHANGELOG.md` | MODIFY | Add new `### Changed — Phase 12: …` entry |
| `README.md` | MODIFY | Drop `"in_type":"any","out_type":"any",` at line 356 |

**Total: 0 new files, 21 modified files.**

## Ordering Constraints

- **None within the slice**: the slice is single-commit. All 21 file changes ship together; ordering is irrelevant because no intermediate commit exists.
- **Kernel before consumers**: within the single diff, kernel changes (manifest.rs, meta.rs, core/mod.rs) must be the foundation; consumer changes (mint.rs, serve.rs, etc.) compile against the new shape. The diff is reviewed as one unit; the file ordering in the diff is alphabetical + by layer, matching `a42573f`'s convention.
- **CHANGELOG entry in same commit**: per research Composite Lesson 9, the entry must be in this commit (not a follow-up). `bd3961c` (Phase 9.5 CHANGELOG backfill) is the precedent for the failure mode.
- **README.md update in same commit**: per Decision 7, the JSON example at line 356 must be updated to match the new payload. Stale README after a payload change is a doc-lie (`4b500e3` is the precedent).

## Verification Notes

- **Compile-time gate**: `cargo build -p odyssey` (catches unused `pub use` re-exports at `core/mod.rs:36-40` and struct field deletions at `manifest.rs:38-58, 106-146`).
- **Test gate**: `cargo test -p odyssey` (resolver + orchestrator + mint tests) and `cargo test -p back` (24 integration tests in `smoke.rs`).
- **Frontend gate**: `pnpm --dir example/fore build` (or its equivalent — checks TS type contract at `types.tsx:24-25, 47-48` after the Rust-side removal).
- **rg-gate (post-merge negative checks)**:
  - `rg '\.host\(' crate/odyssey/src` → 0
  - `rg 'in_type|out_type' crate/odyssey/src example/back/src` → 0
  - `rg 'IsolationMode|HostServiceRef|ResourceHints' crate/odyssey/src` → 0
  - `rg 'manifest\.resources' crate/odyssey/src` → 0
  - `rg '\.host\(' example/back/src` → 0
- **Clippy**: NOT in scope (per Decision 3). If run by CI, the explicit `PluginManifest { plugin, exposes, requires, timeout_ms }` literal in `build()` should pass clippy `needless_update` because every field is listed.
- **Doc-comment drift**: 18 scrub blocks inside `manifest.rs`. None should reference `host`, `in_type`, `out_type`, `isolate`, `IsolationMode`, `ResourceHints`, `HostServiceRef`, `dispatcher`, or `docs/deferred/` (the deferred reference is in the module doc lines 22-26 which are dropped along with the `InProc` sentence).
- **CHANGELOG historical preservation**: lines 210, 626, 649, 864, 867, 869 must remain unchanged (phase-history); the new entry is additive only.
- **docs/deferred/ preservation**: the 5 files (2 readmes, 2 TOMLs, 1 WAT) must remain unchanged.
- **Test coverage impact**: zero functional regression. The 2 affected tests (`:350, :501`) still verify their claims after the literals drop 2 fields each. The other 22 tests are unaffected.

## Performance Considerations

No specific impact. This is a pure deletion refactor that removes dead code and indirection. The post-subtraction `CapabilityMeta` is 11 fields (was 13); the per-call budget lookup is a one-token rename (no algorithmic change). The HTTP bridge emits smaller JSON payloads (2 fewer string fields per capability in 3 wire contracts) — a minor reduction in wire bytes for high-cardinality `/api/caps` responses.

## Migration Notes

- **TOML manifests with old fields fail to parse**: any TOML file referencing `host = [...]`, `in_type = "..."`, `out_type = "..."`, or `[isolate] kind = "in_proc"` will fail to deserialize under the strict serde config (FRD Decision 5). Per the research doc, no TOML is in production use today, so the breakage is theoretical.
- **CHANGELOG entry is mandatory** (research Composite Lesson 9): a multi-commit pass that leaves a CHANGELOG silence is a reviewer-facing regression. The `### Changed — Phase 12: …` entry must be in the same commit as the code changes.
- **No Cargo dependency follow-up**: the subtraction does not orphan any `use` path. `serde_json` survives because `tool_schema: Option<Value>` is the only JSON coupling remaining.

## Pattern References

- `a42573f` (Phase 9 scrub of `.action`/`.protocol`) — `git show a42573f` for the canonical compound-deletion commit pattern (15 files, 70+/440-, single-commit + 4 follow-ups). The upcoming subtraction adopts the same single-commit body structure.
- `9723503` (Phase 8 slim `ResourceHints`) — precedent for the `ResourceHints` flatten, the immediate prior step that left `ResourceHints` as a single-field newtype for this subtraction to finish.
- `0b91cb9` (Phase 9 CI) — clippy `needless_update` precedent; the `..Default::default()` filler stays out of `build()`.
- `bd3961c` (Phase 9.5 CHANGELOG backfill) — the failure mode of multi-commit passes that omit the CHANGELOG entry; the upcoming CHANGELOG entry is in the same commit.
- `4b500e3` (Phase 9 doc-lie fix) — the failure mode of stale doc-comments and README examples after a payload change; the upcoming `README.md:356` update is in the same commit.

## Developer Context

**Q (discover: Pre-resolution 1 — `PluginManifest.host` and `HostServiceRef`):** `PluginManifest.host` is write-only (11 `.host("dispatcher")` call sites across `example/back/src/`, 0 readers anywhere). What scope of deletion?
A: Full delete of `PluginManifest.host` + `HostServiceRef` struct + `ManifestBuilder.host()` + all 11 call sites.

**Q (discover: Pre-resolution 2 — `CapabilityDecl.in_type` and `out_type`):** Dead-via-builder but transitive readers through `CapabilityMeta`. What scope?
A: Full delete + cascade through `CapabilityMeta`, HTTP bridge, profile inspector, agent describe, frontend card.

**Q (discover: Pre-resolution 3 — `PluginManifest.isolate` and `IsolationMode`):** Single-variant enum. What scope?
A: Full delete. Re-introduce when WASM/cdylib loaders ship.

**Q (discover: Pre-resolution 4 — `ResourceHints` flattening):** Single-field newtype. Flatten or keep?
A: Flatten. Inline `timeout_ms: Option<u32>` directly on `PluginManifest`.

**Q (discover: Decision 5 — TOML backwards compat):** How strict?
A: Strict. `#[serde(default)]` retained; old fields fail to parse (acceptable — no TOML in production use).

**Q (discover: Decision 6 — Verification gate):** What commands?
A: `cargo build -p odyssey && cargo test -p odyssey && cargo test -p back`. No `cargo clippy`.

**Q (discover: Decision 7 — Doc comment strategy):** How to update doc-comments?
A: Only clean up dead references. No new explanatory prose.

**Q (research checkpoint: 3 scope-completeness gaps in FRD):** The FRD enumerates a cascade list omitting `meta.rs:48-49` (CapabilityMeta fields), `types.tsx:24-25, 50-51` (TS interfaces), and 2 `CapabilityDecl` struct literals in `smoke.rs:352-353, 503-504`. All silent build breaks. How to handle?
A: All three added to research doc's file scope (developer confirmed at research checkpoint).

**Q (research checkpoint: `core/mod.rs:36-40` re-export collapse):** Stale `pub use` import fails `cargo build -p odyssey`. The re-export is the only non-`manifest.rs` Rust site naming the 3 deleted types. How to handle?
A: Edit `core/mod.rs:14-19, 36-40` in the same commit. Drop `HostServiceRef`, `IsolationMode`, `ResourceHints` from the import list and the module doc.

**Q (research checkpoint: `..Default::default()` filler in `build()`):** Should the post-subtraction `build()` body use `..Default::default()` filler or list every field?
A: List every field explicitly. `..Default::default()` is a clippy `needless_update` candidate (per `0b91cb9` Phase 9 CI).

**Q (design checkpoint: doc-comment scrub strategy):** Strict per Decision 7 (delete-with-field, no new prose), or follow Phase 9's container-paragraph pattern?
A: Strict per Decision 7. Override Phase 9's pattern. No container paragraphs, no `/// Deleted in Phase 12` placeholders.

**Q (design checkpoint: slicing strategy):** 1 slice (single commit), 2 slices (code + docs), or 3 slices (kernel + consumers + docs)?
A: 1 slice. User approved `先做减法`; single commit matches Phase 9 `a42573f` precedent.

## Design History

- Slice 1: Manifest Field Subtraction — approved as generated

## References

- `.rpiv/artifacts/discover/2026-09-16_21-27-12_manifest-field-subtraction.md` — FRD (7 locked decisions, 19 functional requirements)
- `.rpiv/artifacts/research/2026-09-16_22-07-52_manifest-field-subtraction.md` — Research (cascade completeness + 3 scope-completeness gaps surfaced)
- `a42573f` — Phase 9 `.action`/`.protocol` scrub (compound-deletion commit precedent)
- `9723503` — Phase 8 `ResourceHints` slim (immediate prior step)
- `0b91cb9` — Phase 9 CI (clippy `needless_update` precedent)
- `bd3961c` — Phase 9.5 CHANGELOG backfill (the failure mode being avoided)
- `4b500e3` — Phase 9 doc-lie fix (the failure mode being avoided)
- `5dc29e9` — Phase 7 DependencyRef deletion (clean single-commit deletion precedent)
- `218b161` — Phase 9 CapKind unification (field rename cascade precedent)
- `00d1249` — Phase 9.5 resolver dead payload (dead-field side-effect audit precedent)
