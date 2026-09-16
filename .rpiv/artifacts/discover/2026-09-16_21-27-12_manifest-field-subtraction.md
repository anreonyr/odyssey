---
date: 2026-09-16T21:27:12+0800
author: anreonyr
commit: 3c8b0dc
branch: master
repository: odyssey
topic: "Manifest Field Subtraction"
tags: [intent, frd, manifest, plugin, subtraction]
status: ready
last_updated: 2026-09-16T21:27:12+0800
last_updated_by: anreonyr
---

# FRD: Manifest Field Subtraction

## Summary

Subtract every dead or dead-via-builder field from `PluginManifest` and its nested types in `crate/odyssey/src/core/manifest/manifest.rs`. No additions, no structural extensions. Bring the data shape and the `ManifestBuilder` back into alignment so every field has a setter that can express its domain and a reader (or it is removed).

## Problem & Intent

The current `PluginManifest` has accumulated fields that are either `declared-only` (defined and serde-roundtripped but never read) or `dead-via-builder` (the `ManifestBuilder` can only set a single hardcoded value for them, regardless of what the data shape allows). Concretely, four fields are write-only (`PluginManifest.host`, `HostServiceRef.service`, `HostServiceRef.scope`, `PluginManifest.isolate`) and three are dead-via-builder (`CapabilityDecl.in_type`, `CapabilityDecl.out_type`, `CapabilityDecl.contract_name`'s empty-string variant). The `ResourceHints` newtype wraps a single field. The intent — expressed in the developer's own words — is **"先做减法"** ("do subtraction first"): remove the dead code, do not add the speculative fields (`requires.kind`, `optional`, `VersionRange`, `Permissions`, Wasm/Subprocess `IsolationMode` variants, strongly-typed `tool_schema`).

The developer does not have a specific runtime scenario in mind that motivates the cleanup. The motivation is YAGNI-aligned maintenance: each `declared-only` or `dead-via-builder` field is a small lie the type system tells future readers, and the cleanup restores a 1:1 correspondence between "field exists", "field is settable", and "field is read".

## Goals

- Every field on `PluginManifest` and its nested types is reachable via the `ManifestBuilder`, OR it is removed.
- Every field has a real reader in the kernel/resolver/HTTP bridge/runtime, OR it is removed.
- The `ResourceHints` newtype is removed (it is a single-field newtype today).
- `IsolationMode` (single-variant enum) is removed; the deferred WASM/cdylib loaders under `docs/deferred/` will re-introduce it when they ship.
- All `#[serde(default)]` annotations on the manifest stay; the `in_type = "any"` / `host = [...]` patterns that were dead in the spec are now impossible to express.
- Doc comments are scrubbed of references to removed fields; no new prose is added.

## Non-Goals

- Adding `CapabilityRequirement.kind`, `optional`, or `version` range. The resolver does not currently consume them; they will be re-introduced when a consumer is written.
- Extending `IsolationMode` with `Wasm { fuel, memory_mb }` or `Subprocess { ... }` variants. Deferred until those loaders ship.
- Adding a `Permissions` block (`can_spawn_subprocess`, `can_read_fs`, `can_open_network`). Deferred until a host sandbox is in place.
- Strongly typing `tool_schema` (e.g. into a `ToolSchema` enum with `OpenAi { ... }` / `JsonSchema { ... }` variants). The `tool_descriptor` builtin is currently the only reader; defer until a second reader appears.
- Any semantic change to capability resolution, minting, or the runtime.
- Any change to the `PluginId` struct or the `CapKind` enum.

## Functional Requirements

1. The `PluginManifest` struct no longer carries an `isolate: IsolationMode` field.
2. The `PluginManifest` struct no longer carries a `host: Vec<HostServiceRef>` field; the `host` setter is removed from `ManifestBuilder`.
3. The `PluginManifest` struct carries `timeout_ms: Option<u32>` directly (no `ResourceHints` wrapper).
4. The `IsolationMode` enum is removed.
5. The `HostServiceRef` struct is removed.
6. The `ResourceHints` struct is removed.
7. `CapabilityDecl` no longer carries `in_type: String` or `out_type: String`.
8. `CapabilityMeta` no longer carries `in_type: String` or `out_type: String`.
9. `ManifestBuilder::host(...)` is removed; all 11 `.host("dispatcher")` call sites in `example/back/src/` are removed.
10. `ManifestBuilder::build()` constructs `PluginManifest { plugin, exposes, requires, timeout_ms }` and nothing else.
11. The four `expose*` builder methods no longer hardcode `in_type: "any".into()` / `out_type: "any".into()` (the fields are gone).
12. `mint.rs::meta_from_decl` (lines 48-61) no longer copies `in_type` / `out_type` into `CapabilityMeta`.
13. The HTTP bridge payload built in `serve.rs:188-191` no longer includes `in_type` / `out_type` / `streaming` (the bool flatten of `kind`) — wait, `streaming` is derived from `kind` and `kind` is kept; only `in_type` / `out_type` are removed. The HTTP payload is updated to drop those two fields.
14. `example/back/src/profile_inspector.rs:93-102` no longer reads `in_type` / `out_type` from `meta`.
15. `example/back/src/agent.rs:180-183` no longer reads `in_type` / `out_type` from `meta`.
16. `example/fore/src/pages/CapDetail.tsx:113-114` no longer renders `cap.in_type` / `cap.out_type`.
17. `ManifestBuilder`'s `host` field in its internal state is removed.
18. The module-level doc comment in `manifest.rs` is updated to drop the `host services` clause.
19. All other doc comments that reference `host`, `in_type`, `out_type`, `isolate`, `IsolationMode`, `ResourceHints`, or `HostServiceRef` are scrubbed.

## Non-Functional Requirements

- **Performance**: no specific constraint — this is a pure deletion refactor.
- **Security**: no change.
- **UX / Accessibility**: the HTTP `/capabilities` payload (and the `CapDetail.tsx` card) loses the `in_type` and `out_type` fields. The values were always the literal string `"any"`, so the visible behaviour is a removed "always `any`" label — not a regression in functionality.
- **Reliability**: no change. The orchestrator still derives the per-call budget from `manifest.timeout_ms.unwrap_or(5000)` (the call site at `run.rs:233` updates from `manifest.resources.timeout_ms` to `manifest.timeout_ms`).

## Constraints & Assumptions

- The kernel/resolver/HTTP bridge all consume the manifest via the new flat shape. No TOML-driven loader is in production use; the `serde::Deserialize` derive stays on `PluginManifest` and `CapabilityDecl` so a future TOML loader can re-attach.
- `CapabilityDecl.contract_name` is **kept** (it is load-bearing for the resolver contract index at `resolve.rs:145,148,149,151,157`). The empty-string variant it allows in the spec is unreachable via the builder, which the doc comment will note; this is the same doc policy as the existing `manifest.rs:241` comment.
- `CapabilityDecl.kind: CapKind` is **kept**. The `Sync` / `Stream` distinction is load-bearing in the kernel (`run.rs:234`, `serve.rs:188`) and the builder exposes it via method-name choice (`expose` vs `expose_streaming`).
- The 11 `.host("dispatcher")` call sites span: `example/back/src/echo.rs:58`, `reverse.rs:71`, `database.rs:165`, `streaming_echo.rs:144`, `llm.rs:1095`, `memory.rs:506`, `tool_descriptor.rs:169`, `profile_inspector.rs:149`, `agent_runtime.rs:1548`, `agent.rs:295`, `agent.rs:340`. Each will be removed as part of FR9.
- The deferred WASM / cdylib / subprocess loaders in `docs/deferred/` are out of scope for this FRD. When those loaders return, the `IsolationMode` enum returns alongside them, with `Wasm` and `Subprocess` variants.

## Acceptance Criteria

- [ ] `cargo build -p odyssey` exits 0.
- [ ] `cargo test -p odyssey` exits 0.
- [ ] `cargo test -p back` exits 0 (covers the 11 manifest-construction call sites in `example/back/tests/smoke.rs` and the manifest-construction in builtin plugin modules).
- [ ] `rg "HostServiceRef" crate/odyssey/src example/back/src` returns no matches.
- [ ] `rg "IsolationMode|InProc" crate/odyssey/src/core/manifest` returns no matches.
- [ ] `rg "ResourceHints" crate/odyssey/src` returns no matches.
- [ ] `rg "\.host\(" example/back/src` returns no matches.
- [ ] `rg "in_type|out_type" crate/odyssey/src example/back/src` returns no matches.
- [ ] `rg "CapabilityMeta.*in_type|CapabilityMeta.*out_type" crate/odyssey/src` returns no matches.
- [ ] `manifest.rs` and `ManifestBuilder` are in lockstep: every `PluginManifest` field is settable by the builder, and every builder method sets a field that exists on the resulting `PluginManifest`.
- [ ] The diff in `manifest.rs` removes 4 field declarations (`isolate`, `host`, `resources`, plus 2 nested-struct fields on `CapabilityDecl`), 1 enum (`IsolationMode`), 2 structs (`HostServiceRef`, `ResourceHints`), and 1 builder method (`.host`).

## Recommended Approach

A pure deletion refactor concentrated in `crate/odyssey/src/core/manifest/manifest.rs`, with cascading edits in `crate/odyssey/src/personality/lifecycle/mint.rs`, `crate/odyssey/src/personality/lifecycle/serve.rs`, the example backend plugins under `example/back/src/`, the frontend card under `example/fore/src/pages/CapDetail.tsx`. No new abstractions, no moved code, no API additions. The downstream `research` skill will validate the cascading-reader list and confirm no third-party consumer outside `example/` references the removed fields.

## Decisions

### Pre-resolution 1 — `PluginManifest.host` and `HostServiceRef`

**Question**: `PluginManifest.host` is write-only (11 `.host("dispatcher")` call sites across `example/back/src/`, 0 readers anywhere in `crate/odyssey/src` or `example/back/src`). What scope of deletion?
**Recommended**: Full delete of `PluginManifest.host` + `HostServiceRef` struct + `ManifestBuilder.host()` + all 11 call sites.
**Chosen**: Full delete.
**Rationale**: User intent: "先做减法". A write-only field with no consumer is the canonical "declared-only" smell. The conservative "delete only `HostServiceRef::scope`" option was rejected because `service` is also write-only, and would require a second cleanup pass.

### Pre-resolution 2 — `CapabilityDecl.in_type` and `out_type`

**Question**: `in_type` and `out_type` are `dead-via-builder` (every `expose*` builder hardcodes `"any"` at `manifest.rs:252,253,270,271,293,294,314,315`) but have transitive readers through `CapabilityMeta` (mint at `mint.rs:56,57`, HTTP bridge at `serve.rs:190,191`, profile inspector at `profile_inspector.rs:95,96`, agent describe at `agent.rs:182,183`, frontend card at `CapDetail.tsx:113,114`). What scope of deletion?
**Recommended**: Full delete + cascade. Remove from `CapabilityDecl`, `CapabilityMeta`, the HTTP payload, the profile inspector JSON, the agent describe JSON, the frontend card.
**Chosen**: Full delete + cascade.
**Rationale**: User intent: "先做减法". The only value ever set was `"any"`, so the cascade is a label removal in the JSON / UI — no functional regression. The "add a builder setter" option was rejected as it would be a structural addition conflicting with the stated subtraction scope.

### Pre-resolution 3 — `PluginManifest.isolate` and `IsolationMode`

**Question**: `PluginManifest.isolate: IsolationMode` is a single-variant enum; the only reader of the variant is the deferred `docs/deferred/` WASM/cdylib design (which has not shipped). What scope of deletion?
**Recommended**: Full delete. `IsolationMode` and the `isolate` field are gone; re-introduce the enum (with `Wasm` / `Subprocess` variants) when those loaders ship.
**Chosen**: Full delete.
**Rationale**: User intent: "先做减法". The "keep as placeholder" option preserves a serde footprint for a future that has not materialised, paying the cost of indirection today for a speculative payoff. The "keep enum but `#[allow(dead_code)]` internal" option still pays the type-system cost without providing any current value.

### Pre-resolution 4 — `ResourceHints` flattening

**Question**: `ResourceHints` (`manifest.rs:156`) wraps only `timeout_ms: Option<u32>`. Flatten or keep the newtype as a forward-compat slot?
**Recommended**: Flatten. Inline `timeout_ms: Option<u32>` directly on `PluginManifest`; delete the `ResourceHints` struct.
**Chosen**: Flatten.
**Rationale**: User intent: "先做减法" (developer explicitly said "ResourceHints 拍平" in the prior conversation). A single-field newtype is an anti-pattern under YAGNI; reintroduce `ResourceHints` the day a second resource hint ships with a real reader.

### Decision 5 — TOML backwards compatibility

**Question**: No TOML manifest is in production use today, but the `serde::Deserialize` derive is kept on `PluginManifest` and `CapabilityDecl` so a future TOML loader can re-attach. How strict should the schema be?
**Recommended**: Strict — retain `#[serde(default)]` on the remaining `Vec` / `Option` fields. Any existing TOML file referencing `host = [...]` or `in_type = "..."` would fail to parse, which is acceptable given no TOML is in production use.
**Chosen**: Strict.
**Rationale**: The "lenient" option (silently drop unknown fields) is the current behaviour and is exactly the "deny_unknown_fields = false" smell that allowed the dead fields to accumulate without a warning. A parse error on stale TOML is a clearer signal than silent field drop.

### Decision 6 — Verification gate

**Question**: What commands must pass after the subtraction to consider the change done?
**Recommended**: `cargo build -p odyssey && cargo test -p odyssey && cargo test -p back`. The `-p back` test run is the meaningful gate because it exercises manifest construction paths via the example plugins.
**Chosen**: Full build+test.
**Rationale**: The orchestrator / resolver / HTTP bridge tests live in `odyssey`; the manifest-construction paths (the call sites that need updating) live in `back`. Both must pass.

### Decision 7 — Doc comment strategy

**Question**: How should doc comments be updated to reflect the new shape?
**Recommended**: Update the module-level doc + scrub all doc comments that reference removed fields. Do **not** add new prose (the developer's preference for "只清理死的").
**Chosen**: Only clean up dead references.
**Rationale**: User intent: "只清理死的". Strip references to `host services`, `IsolationMode`, `ResourceHints`, `HostServiceRef`, `in_type`, `out_type` from existing doc comments; do not author new explanatory prose in this change.

## Open Questions

None. All interview branches have a Decision.

## Suggested Follow-ups

- **Re-introduce `requires.kind` / `optional` / `VersionRange`** when the resolver starts enforcing type-compatible bindings and graceful-degradation paths. The current resolver (`resolve.rs:139-374`) only matches on `contract` name; adding `kind` enforcement will require a new struct field on `CapabilityRequirement` and a new builder method.
- **Re-introduce `IsolationMode` with `Wasm` / `Subprocess` variants** when the deferred WASM / cdylib / subprocess loaders under `docs/deferred/` ship. The enum will return with multiple variants and the `ManifestBuilder` will need a corresponding `isolate(...)` setter.
- **Add a `Permissions` block** to `PluginManifest` when a host sandbox is in place to consume it (`can_spawn_subprocess`, `can_read_fs`, `can_open_network`). Without a sandbox consumer, adding the fields would replicate the `in_type` / `host` dead-field pattern.
- **Strongly type `tool_schema`** (e.g. into a `ToolSchema` enum with `OpenAi { ... }` / `JsonSchema { ... }` variants) when a second consumer beyond `tool_descriptor` needs to interpret it.
- **Add a host-services consumer** if the dispatcher / host service registry ever needs to introspect `host` declarations. Today every builtin declares `.host("dispatcher")` and no code reads it (`manifest.rs:55`, `HostServiceRef` def at `manifest.rs:149`).

## References

- Source file: `crate/odyssey/src/core/manifest/manifest.rs` (lines 38-368)
- Module: `crate/odyssey/src/core/manifest/mod.rs`
- Resolver (kept): `crate/odyssey/src/personality/composition/resolve.rs:139-160, 340-374`
- Orchestrator (kept; `resources.timeout_ms` reader at `run.rs:233`): `crate/odyssey/src/personality/lifecycle/run.rs:195, 208, 231-234`
- Mint (cascading edits at lines 48-61): `crate/odyssey/src/personality/lifecycle/mint.rs`
- HTTP bridge (cascading edits at lines 188-191): `crate/odyssey/src/personality/lifecycle/serve.rs`
- Example backend readers (cascading edits at lines 93-102, 180-183): `example/back/src/profile_inspector.rs`, `example/back/src/agent.rs`
- Frontend card (cascading edits at lines 113-114): `example/fore/src/pages/CapDetail.tsx`
- 11 host call sites in: `example/back/src/{echo.rs:58, reverse.rs:71, database.rs:165, streaming_echo.rs:144, llm.rs:1095, memory.rs:506, tool_descriptor.rs:169, profile_inspector.rs:149, agent_runtime.rs:1548, agent.rs:295, agent.rs:340}`
- Probe reports:
  - `codebase-locator` (field reader inventory) — 2026-09-16_21-27-12
  - `codebase-analyzer` (builder ↔ data shape alignment) — 2026-09-16_21-27-12
- Prior conversation: `字段设计的怎么样?` / `理想的 manifest 应该有什么?` reviews that motivated this FRD.
