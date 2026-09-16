---
date: 2026-09-16T22:07:52+0800
author: anreonyr
commit: 3c8b0dc
branch: master
repository: odyssey
topic: "Manifest Field Subtraction Cascade"
tags: [research, codebase, manifest, plugin, subtraction, cascade]
status: ready
last_updated: 2026-09-16T22:07:52+0800
last_updated_by: anreonyr
---

# Research: Manifest Field Subtraction Cascade

## Research Question

Verify the FRD's "full delete + cascade" scope for the `PluginManifest` field subtraction is complete — specifically: that the cascading-reader list enumerated in the FRD is the *complete* list, that no third-party consumer outside `example/` references the removed types, and that the deferred `docs/deferred/` designs are truly dormant. The research artefact is consumed by `/skill:design` to author the implementation design and by `/skill:plan` to phase the work.

Source topic: `.rpiv/artifacts/discover/2026-09-16_21-27-12_manifest-field-subtraction.md` (FRD with 7 locked decisions, 19 functional requirements, 0 open questions).

## Summary

The FRD's deletion cascade captures the **core** sites but is **incomplete on three axes** that surface as silent build breaks:

1. `crate/odyssey/src/core/meta/meta.rs:48-49` (`CapabilityMeta.in_type` / `out_type`) — the FRD's FR8 mentions the deletion but does not name the file. `mint.rs:56-57` clones the values from `decl` to `meta`; if `meta.rs` is not updated, the assignment fails to compile.
2. `example/fore/src/api/types.tsx:24-25, 50-51` (TypeScript interfaces `CapInfo.in_type` / `out_type` and `AgentHandleDescribeLive.in_type` / `out_type`) — not in the FRD's cascade list. The frontend TS type contract is hand-written (not generated), so a Rust-side field removal without a TS-side update triggers `tsc` errors that block `pnpm build`.
3. `example/back/tests/smoke.rs:352-353, 503-504` (`CapabilityDecl` struct literals in `agent_core_describes_handles` and `agent_describe_refuses_unknown_fields` tests) — not enumerated as call sites. The `cargo test -p back` gate will fail with "no field `in_type` on type `CapabilityDecl`" until these are updated.

All three are now in scope (developer confirmed during checkpoint). The full cascade table below covers 16 files across 4 layers (kernel, personality, example backend, frontend), 3 JSON wire contracts (`CapInfo` HTTP, `agent_describe` HTTP, `profile_inspect` HTTP), 11 `.host("dispatcher")` builder call sites, 2 `CapabilityDecl` test literals, and 18 doc-comment scrub blocks inside `manifest.rs`.

The 5 prior phase cleanups (Phase 5 D5, Phase 7 DependencyRef, Phase 8 `9723503` ResourceHints slim, Phase 9 `a42573f` .action/.protocol scrub, Phase 9.5 `00d1249` resolver dead-payload) all follow the same pattern. The top lesson: **delete the writer, the reader, and the warnings in one commit**; orphaning any of the three is what every prior phase had to follow up to repair.

## Detailed Findings

### Layer 1: Kernel — `crate/odyssey/src/core/`

#### `manifest.rs` (target)

Six field-level deletions + one new field:

| Field | Line | Type | Pre-subtraction builder status | Post-subtraction |
| --- | --- | --- | --- | --- |
| `plugin` | `manifest.rs:39` | `PluginId` | settable via `new()` + `version()` | unchanged |
| `isolate` | `manifest.rs:43` | `IsolationMode` | hardcoded `InProc` at `build()` (:362) — **dead-via-builder** | **DELETED** (FR1, FR4) |
| `exposes` | `manifest.rs:45` | `Vec<CapabilityDecl>` | settable via 4 `expose*` methods | unchanged |
| `requires` | `manifest.rs:53` | `Vec<CapabilityRequirement>` | settable via `requires()` | unchanged |
| `host` | `manifest.rs:55` | `Vec<HostServiceRef>` | settable via `.host()` but **zero readers** anywhere | **DELETED** (FR2, FR5) |
| `resources` | `manifest.rs:57` | `ResourceHints` (single-field newtype) | settable via `.timeout_ms()` | **REPLACED** by `timeout_ms: Option<u32>` directly on `PluginManifest` (FR3, FR6) |

Plus cascading internal-state changes:

- `ManifestBuilder` struct at `manifest.rs:209-218` — drop the `host: Vec<HostServiceRef>` field at `:212` and the `host: Vec::new()` initialiser at `:227`.
- `ManifestBuilder::host()` method at `manifest.rs:333-340` — deleted.
- `ManifestBuilder::build()` at `manifest.rs:351-368` — drop `host` from destructure (`:358, :365`), drop `isolate: IsolationMode::InProc` (`:362`), replace `resources: ResourceHints { timeout_ms }` (`:366`) with `timeout_ms,`.
- Four `expose*` builder methods (`manifest.rs:247-321`) — drop `in_type: "any".into()` and `out_type: "any".into()` from each struct literal (`:250-251, :271-272, :292-293, :314-315`).
- `IsolationMode` enum at `manifest.rs:99-103` — deleted (single-variant, no reader).
- `HostServiceRef` struct at `manifest.rs:149-153` — deleted.
- `ResourceHints` struct at `manifest.rs:155-167` — deleted.

Post-subtraction `PluginManifest` shape: 4 fields (`plugin`, `exposes`, `requires`, `timeout_ms`), each with a builder method. The 1:1 setter↔field invariant from the FRD's Acceptance Criterion holds.

#### `core/mod.rs:14-19, 36-40` (re-exports, missing from FRD scope as explicit site)

The `pub use manifest::manifest::{...}` block at `:36-40` and the module doc at `:14-19` are the **single convergence point** for the three deleted types. After subtraction the block collapses:

```rust
// before
pub use manifest::manifest::{
    CapabilityDecl, CapabilityRequirement, HostServiceRef, IsolationMode, ManifestBuilder,
    PluginManifest, ResourceHints,
};
// after
pub use manifest::manifest::{
    CapabilityDecl, CapabilityRequirement, ManifestBuilder, PluginManifest,
};
```

A stale `pub use` import will fail `cargo build -p odyssey`. This is the only Rust site outside `manifest.rs` that names `HostServiceRef`, `IsolationMode`, or `ResourceHints`.

#### `core/meta/meta.rs:48-49` (missing from FRD file scope — `CapabilityMeta` is in this file)

`CapabilityMeta` mirrors `CapabilityDecl` and is the runtime struct stamped at mint time. The `in_type` and `out_type` fields at `meta.rs:48-49` are the only reader of the values written by the four `expose*` builder methods (`CapabilityDecl.in_type`, `CapabilityDecl.out_type`). After the FR's `CapabilityDecl` deletion, `mint.rs:56-57` (`meta_from_decl`) must drop its `in_type: decl.in_type.clone()` and `out_type: decl.out_type.clone()` writes, and `meta.rs` must drop the field declarations. If `meta.rs` is not updated, the assignment `in_type: decl.in_type.clone()` fails to compile (the source field no longer exists).

This is the FRD's FR8 referenced at the type level but not the file level. **Confirmed in scope** by developer checkpoint.

### Layer 2: Personality — `crate/odyssey/src/personality/`

#### `lifecycle/mint.rs:48-61` (`meta_from_decl`)

`CapabilityMeta` is built from a `CapabilityDecl` here. The function at lines 42-67 is the single mint-side propagator: every `Capability<R>` minted by `CapabilityFactory::mint` (`:119-135`) goes through it. Today it copies `in_type` and `out_type` at lines 56-57. After subtraction, both lines are removed. The `namespace_for` and `CapabilityFactory` doc-comments at `:33-39, :65-68, :83-92` do not reference removed fields and stay unchanged.

#### `lifecycle/serve.rs:67-77, 184-194` (HTTP bridge `CapInfo`)

`CapInfo` struct (`:67-77`) is the cross-language wire contract for `/api/caps`. The `streaming: bool` field at `:73` is derived from `meta.kind == CapKind::Stream` and **stays** (load-bearing for HTTP API back-compat per the doc comment at `:71-72`). The `in_type: String` and `out_type: String` fields at `:75-76` are deleted. The `list_caps` function (`:184-194`) drops the `in_type: m.in_type.clone()` and `out_type: m.out_type.clone()` writes at `:195-196`.

The 1:1 cross-language mapping table (Rust struct → JSON key → TS interface):

| Rust (`serve.rs:CapInfo`) | JSON key | TS (`types.tsx:CapInfo`) | TS render site |
| --- | --- | --- | --- |
| `name: String` (`:69`) | `"name"` | `name: string` (`:17`) | `Caps.tsx:147`, `CapDetail.tsx:84` |
| `id: String` (`:70`) | `"id"` | `id: string` (`:19`) | `CapDetail.tsx:110` |
| `streaming: bool` (`:73`) | `"streaming"` | `streaming: boolean` (`:21`) | `Caps.tsx:149`, `CapDetail.tsx:88` |
| `timeout_ms: u32` (`:74`) | `"timeout_ms"` | `timeout_ms: number` (`:23`) | `Caps.tsx:156`, `CapDetail.tsx:112` |
| **`in_type: String` (`:75`)** | `"in_type"` | **`in_type: string` (`:24`)** | **`CapDetail.tsx:113`** |
| **`out_type: String` (`:76`)** | `"out_type"` | **`out_type: string` (`:25`)** | **`CapDetail.tsx:114`** |

#### `lifecycle/run.rs:229-234` (per-call budget, `ResourceHints` consumer)

`run.rs:230` reads `entry.manifest.resources.timeout_ms.unwrap_or(5000)` and passes it to `CapabilityBudget::new(...)`. After `ResourceHints` flatten, this becomes `entry.manifest.timeout_ms.unwrap_or(5000)` — a one-token rename. The only live reader of `manifest.resources` in the entire repo.

### Layer 3: Example backend — `example/back/`

#### 11 `.host("dispatcher")` call sites (FR9)

All 11 sites are inside `impl BuiltinManifest for XxxBuiltin { fn manifest(&self) -> PluginManifest { ... } }`. The pattern is `ManifestBuilder::new(...).expose*(...).host("dispatcher").timeout_ms(...).build()`. `agent.rs` carries **two** of the eleven (lines 295 and 340) because it defines two builtins (`AgentListBuiltin` and `AgentDescribeBuiltin`); the FRD correctly notes this.

| # | File | Line | Builtin |
| --- | --- | --- | --- |
| 1 | `example/back/src/echo.rs` | `:58` | `EchoBuiltin` |
| 2 | `example/back/src/reverse.rs` | `:71` | `ReverseBuiltin` |
| 3 | `example/back/src/database.rs` | `:165` | `DatabaseBuiltin` |
| 4 | `example/back/src/streaming_echo.rs` | `:144` | `StreamingEchoBuiltin` |
| 5 | `example/back/src/llm.rs` | `:1095` | `LlmBuiltin` |
| 6 | `example/back/src/memory.rs` | `:506` | `MemoryBuiltin` |
| 7 | `example/back/src/tool_descriptor.rs` | `:169` | `ToolDescriptorBuiltin` |
| 8 | `example/back/src/profile_inspector.rs` | `:149` | `ProfileInspectorBuiltin` |
| 9 | `example/back/src/agent_runtime.rs` | `:1548` | `AgentRuntimeBuiltin` |
| 10 | `example/back/src/agent.rs` | `:295` | `AgentListBuiltin` |
| 11 | `example/back/src/agent.rs` | `:340` | `AgentDescribeBuiltin` |

`agent.rs` has two entries because it defines two `BuiltinManifest` impls. All 11 lines delete the `.host("dispatcher")` call (the surrounding `ManifestBuilder::new(...).expose*(...).timeout_ms(...).build()` stays).

#### `profile_inspector.rs:87-105` (the `inspect` builtin)

`ProfileInspectorResource::inspect` at `:87-105` builds a JSON envelope. The `in_type` / `out_type` reads at `:95-96` are deleted. This is a separate JSON wire contract (not consumed by the React UI, but emitted on the HTTP surface). The module doc at `:5-23` already doesn't list `in_type` / `out_type` (it says "subject, name, contract, kind, budget, quota, operations"), so no doc-comment scrub is needed here.

#### `agent.rs:160-188` (`AgentCore::describe` for `agent_describe`)

`AgentCore::describe` at `:160-188` is the third JSON wire contract. The `in_type: meta.in_type` and `out_type: meta.out_type` JSON-key writes at `:185-186` are deleted. The `streaming: meta.kind == CapKind::Stream` line at `:184` stays. The module doc at `:1-41` doesn't reference `in_type` / `out_type` and stays unchanged.

#### `tests/smoke.rs:350-357, 501-508` (missing from FRD scope — `CapabilityDecl` struct literals)

`example/back/tests/smoke.rs:350-357` (inside `agent_core_describes_handles`):

```rust
let plain_decl = CapabilityDecl {
    name: "plain".into(),
    in_type: "any".into(),     // :352 — DELETE
    out_type: "any".into(),    // :353 — DELETE
    kind: CapKind::Sync,
    contract_name: "plain".into(),
    tool_schema: None,
};
```

`example/back/tests/smoke.rs:501-508` (inside `agent_describe_refuses_unknown_fields`):

```rust
let decl = CapabilityDecl {
    name: "echo".into(),
    in_type: "any".into(),     // :503 — DELETE
    out_type: "any".into(),    // :504 — DELETE
    kind: CapKind::Sync,
    contract_name: "echo".into(),
    tool_schema: None,
};
```

These literals directly construct the post-deletion shape from the module under test. The compiler will reject each with a "no field `in_type` on type `CapabilityDecl`" / "no field `out_type` on type `CapabilityDecl`" pair. `cargo test -p back` will fail at compile, not at test-run.

**Confirmed in scope** by developer checkpoint.

### Layer 4: Frontend — `example/fore/`

#### `api/types.tsx:24-25, 50-51` (missing from FRD scope — TypeScript interfaces)

Two hand-written TS interfaces mirror the Rust `CapInfo` and `agent_describe` JSON contracts. The hand-written design (per the doc comment at `types.tsx:1-9`) is what makes the missing-fields test meaningful — without the explicit TS types, a Rust-side field removal would silently leave an unused `cap.in_type` reference in JSX.

`types.tsx:15-28` (`CapInfo`):

```typescript
export interface CapInfo {
  name: string;
  id: string;
  streaming: boolean;
  timeout_ms: number;
  in_type: string;        // :24 — DELETE
  out_type: string;       // :25 — DELETE
}
```

`types.tsx:36-51` (`AgentHandleDescribeLive`):

```typescript
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
  in_type: string;        // :47 — DELETE
  out_type: string;       // :48 — DELETE
  timeout_ms: number;
  calls_per_minute: number;
  operations: Array<"READ" | "WRITE" | "EXECUTE" | "ADMIN">;
}
```

**Confirmed in scope** by developer checkpoint.

#### `pages/CapDetail.tsx:113-114` (the only JSX render site)

```tsx
<Meta label="in" value={cap.in_type} mono />
<Meta label="out" value={cap.out_type} mono />
```

Both lines delete. The surrounding `<Separator />` at `:112` and the other `<Meta>` rows stay. The `<Meta>` helper at `CapDetail.tsx:251-268` is field-agnostic, so no helper changes.

#### Frontend files that read but don't render (no edit needed)

- `pages/Invoke.tsx`, `pages/Caps.tsx`, `hooks/useCaps.tsx`, `api/client.tsx` — all import `CapInfo` as a type but never read `in_type` / `out_type`. They pass `CapInfo[]` through to consumers. No edit needed.

### Documentation Layer

#### `manifest.rs` doc-comment scrub list (18 blocks, FR19)

Every doc-comment in `manifest.rs` is classified by attachment:

| Block | File:Line | Attachment | Action |
| --- | --- | --- | --- |
| 1 | `manifest.rs:18-26` | Module doc | Drop `host services` and `InProc isolation` and `docs/deferred/` clauses |
| 2 | `manifest.rs:40-42` | Field doc on `isolate` | Delete with field |
| 3 | `manifest.rs:55` | Field `host` on `PluginManifest` | Delete field; no doc |
| 4 | `manifest.rs:57` | Field `resources` on `PluginManifest` | Replace field; no doc |
| 5 | `manifest.rs:89-99` | Outer doc on `enum IsolationMode` | Delete with enum |
| 6 | `manifest.rs:108-110` | Field doc on `in_type` | Delete with field |
| 7 | `manifest.rs:113-115` | Field doc on `out_type` | Delete with field |
| 8 | `manifest.rs:155-167` | `ResourceHints` struct + `timeout_ms` doc | Delete struct; relocate `timeout_ms` doc to new flat field |
| 9 | `manifest.rs:182-191` | `ManifestBuilder` defaults table | Drop rows for `in_type` (`:183`), `out_type` (`:184`), `host` (`:188`); update `timeout_ms` row (`:191`) text if needed |
| 10 | `manifest.rs:197-206` | `ManifestBuilder` historical-narrative paragraph about singular setters | Delete paragraph (now stale) |
| 11 | `manifest.rs:212` | Internal field `host` on `ManifestBuilder` | Delete field; no doc |
| 12 | `manifest.rs:242-246` | `expose` method doc — paragraph about `in_type`/`out_type` stay `"any"` | Delete paragraph |
| 13 | `manifest.rs:259-262` | `expose_with_schema` method doc | No scrub (no removed-field ref) |
| 14 | `manifest.rs:280-284` | `expose_streaming` method doc | No scrub (no removed-field ref) |
| 15 | `manifest.rs:301-305` | `expose_streaming_with_schema` method doc | No scrub (no removed-field ref) |
| 16 | `manifest.rs:333-334` | `host` method doc | Delete with method |
| 17 | `manifest.rs:323-324` | `requires` method doc | No scrub |
| 18 | `manifest.rs:350` | `build` method doc | No scrub (generic) |

The relocated `timeout_ms` doc on the new flat field (`PluginManifest.timeout_ms`) keeps the L157-158 lead sentence "Per-call wall-clock budget in milliseconds. The only field the kernel actually uses." but truncates the L160-165 Phase 8 cleanup paragraph (it documents the deletion of fields that no longer exist anywhere). Per Decision 7 ("only clean up dead references"), no new prose is added.

#### `README.md:356` (current-state example, must update)

`README.md:344-359` is the live `## Agent builtin` section. Line 356:

```
"streaming":false,"in_type":"any","out_type":"any",
```

The `streaming` stays (derived from kept `kind`). `in_type` and `out_type` are removed. The new line is:

```
"streaming":false,
```

#### `CHANGELOG.md:210, 626, 649, 864, 867, 869` (historical phase notes, must NOT update)

All six references are inside dated phase-history sections (`### Added — agent builtin: ...` at L189, `### Breaking — Phase 8` at L544, `#### Manifest builder (src/kernel/manifest_builder.rs)` at L845 under `### Added — Phase 3`). Each documents the manifest shape *as it was in that phase*, not as it is today. The CHANGELOG treats past phases as immutable history (see the phase-section structure starting at L189 and the section dating convention).

`CHANGELOG.md:859, 865-866, 869` are also historical (Phase 3 builder example showing `.host("dispatcher")` in a fenced code block at L857-861, etc.) and stay unchanged.

**A new `CHANGELOG.md` entry for this subtraction is mandatory**, not optional. `bd3961c` (Phase 9.5) had to backfill a Phase 9 entry that the multi-commit pass had silently omitted. The new entry enumerates: 4 fields deleted (`isolate`, `host`, `resources`, plus `in_type`/`out_type` on `CapabilityDecl`), 1 enum deleted (`IsolationMode`), 2 structs deleted (`HostServiceRef`, `ResourceHints`), 1 builder method deleted (`.host`), 5 files in `example/back/` updated, 1 frontend file (`CapDetail.tsx`) updated, 1 TS types file (`types.tsx`) updated.

#### `docs/deferred/` (dormant, out of scope)

Five files:

- `wasm-loader-readme.md` (lines 1-20 explicitly state "predates Phase 8 and Phase 9", "Do not read the TOML as a manifest the current code accepts")
- `wasm-loader.toml` (TOML sketch with `in_type = "any"`, `out_type = "any"`, `[host] service = "dispatcher"`)
- `wasm-loader.wat` (real WebAssembly text module — `(module ...)` at L15, `(memory (export "memory") 1)` at L17, `name` function at L24, `invoke` function at L52)
- `cdylib-loader-readme.md` (parallel to wasm readme)
- `cdylib-loader.toml` (parallel to wasm TOML)

Verified dormant: `grep "from_toml_str|from_path|include_str!|include_bytes!" crate/odyssey/src` returns zero matches outside `manifest.rs:9-10` (the Phase 9 doc-comment that *documents* the deleted loader). `grep "use\s+.*deferred"` repo-wide returns zero matches. No `Cargo.toml` lists any `docs/deferred/*` file. The `.wat` is not embedded by any source file.

**Out of scope** for this subtraction. The TOML sketches deliberately reference the soon-to-be-removed shapes — they document a future format that will re-introduce the fields under different semantics when the WASM/cdylib loaders ship. An over-eager scrub would destroy the design history they encode.

## Code References

Jump-table for the planner. Format: `path:startLine-endLine` — purpose.

### Kernel

- `crate/odyssey/src/core/manifest/manifest.rs:38-58` — `PluginManifest` struct (target of the refactor; 6 fields, 3 to delete + 1 to flatten)
- `crate/odyssey/src/core/manifest/manifest.rs:75-87` — `CapabilityRequirement` (unchanged; 2 fields)
- `crate/odyssey/src/core/manifest/manifest.rs:99-103` — `IsolationMode` enum (single variant; deleted)
- `crate/odyssey/src/core/manifest/manifest.rs:106-146` — `CapabilityDecl` struct (6 fields; `in_type` and `out_type` deleted)
- `crate/odyssey/src/core/manifest/manifest.rs:149-153` — `HostServiceRef` struct (2 fields; deleted)
- `crate/odyssey/src/core/manifest/manifest.rs:155-167` — `ResourceHints` struct (single field; deleted; `timeout_ms` doc relocated to flat field)
- `crate/odyssey/src/core/manifest/manifest.rs:209-368` — `ManifestBuilder` (drop `host` setter + `host` field; update `build()` to drop `host` from destructure, drop `isolate: IsolationMode::InProc`, replace `resources: ResourceHints { timeout_ms }` with `timeout_ms`)
- `crate/odyssey/src/core/manifest/manifest.rs:247-321` — Four `expose*` builder methods (drop `in_type` / `out_type` literals at `:250-251, :271-272, :292-293, :314-315`)
- `crate/odyssey/src/core/mod.rs:14-19` — Module doc listing `manifest` submodule exports (drop `HostServiceRef`, `IsolationMode`, `ResourceHints`)
- `crate/odyssey/src/core/mod.rs:36-40` — `pub use manifest::manifest::{...}` re-export block (drop 3 tokens, collapses to 4-token list)
- `crate/odyssey/src/core/meta/meta.rs:48-49` — `CapabilityMeta.in_type` / `out_type` fields (deleted; mirror of `CapabilityDecl`)

### Personality

- `crate/odyssey/src/personality/lifecycle/mint.rs:42-67` — `meta_from_decl` (drop `in_type` / `out_type` clone sites at `:56-57`)
- `crate/odyssey/src/personality/lifecycle/serve.rs:67-77` — `CapInfo` struct (drop `in_type` / `out_type` at `:75-76`; `streaming` stays)
- `crate/odyssey/src/personality/lifecycle/serve.rs:184-194` — `list_caps` (drop `in_type` / `out_type` clone sites at `:195-196`)
- `crate/odyssey/src/personality/lifecycle/run.rs:229-234` — Orchestrator mint loop (rename `entry.manifest.resources.timeout_ms` to `entry.manifest.timeout_ms` at `:230`)

### Example backend

- `example/back/src/echo.rs:58` — `.host("dispatcher")` call site (delete)
- `example/back/src/reverse.rs:71` — same
- `example/back/src/database.rs:165` — same
- `example/back/src/streaming_echo.rs:144` — same
- `example/back/src/llm.rs:1095` — same
- `example/back/src/memory.rs:506` — same
- `example/back/src/tool_descriptor.rs:169` — same
- `example/back/src/profile_inspector.rs:149` — same
- `example/back/src/agent_runtime.rs:1548` — same
- `example/back/src/agent.rs:295` — `AgentListBuiltin`'s `.host("dispatcher")` (delete)
- `example/back/src/agent.rs:340` — `AgentDescribeBuiltin`'s `.host("dispatcher")` (delete)
- `example/back/src/profile_inspector.rs:87-105` — `ProfileInspectorResource::inspect` (drop `in_type` / `out_type` at `:95-96`)
- `example/back/src/agent.rs:160-188` — `AgentCore::describe` (drop `in_type` / `out_type` at `:185-186`)
- `example/back/tests/smoke.rs:350-357` — `CapabilityDecl` struct literal in `agent_core_describes_handles` (drop `in_type` / `out_type` at `:352-353`)
- `example/back/tests/smoke.rs:501-508` — `CapabilityDecl` struct literal in `agent_describe_refuses_unknown_fields` (drop `in_type` / `out_type` at `:503-504`)

### Frontend

- `example/fore/src/api/types.tsx:15-28` — `CapInfo` interface (drop `in_type` / `out_type` at `:24-25`)
- `example/fore/src/api/types.tsx:36-51` — `AgentHandleDescribeLive` interface (drop `in_type` / `out_type` at `:47-48`)
- `example/fore/src/pages/CapDetail.tsx:113-114` — `<Meta>` JSX for `in_type` / `out_type` (delete both lines)

### Documentation

- `crate/odyssey/src/core/manifest/manifest.rs:1-30` — Module doc (drop `host services` clause at `:18-20`, drop `InProc` sentence at `:22-23`, drop `docs/deferred/` clause at `:24-25`)
- `crate/odyssey/src/core/manifest/manifest.rs:182-191` — `ManifestBuilder` defaults table (drop rows `:183, :184, :188`)
- `crate/odyssey/src/core/manifest/manifest.rs:197-206` — Stale historical-narrative paragraph (delete)
- `crate/odyssey/src/core/manifest/manifest.rs:242-246` — `expose` method doc paragraph (delete `:242-246`)
- `README.md:356` — Current-state JSON example (drop `"in_type":"any","out_type":"any",`)

## Integration Points

### Inbound References (who reads the manifest)

- `crate/odyssey/src/personality/composition/resolve.rs:139-160` — Resolver reads `m.plugin`, `m.exposes[].contract_name`, builds contract index
- `crate/odyssey/src/personality/composition/resolve.rs:340-374` — Resolver reads `m.requires[].contract` / `m.requires[].name`, builds edges + bindings
- `crate/odyssey/src/personality/lifecycle/mint.rs:42-67` — Mint reads `decl.name`, `decl.contract_name`, `decl.kind`, `decl.tool_schema` (and currently `decl.in_type` / `decl.out_type` which are removed)
- `crate/odyssey/src/personality/lifecycle/run.rs:195, 208, 231-234` — Orchestrator reads `manifest.plugin.name` (registry key), `manifest.exposes.len()` + iter, `manifest.timeout_ms` (per-call budget)
- `crate/odyssey/src/personality/lifecycle/serve.rs:184-194` — HTTP bridge reads `meta.in_type` / `meta.out_type` (both removed)
- `example/back/src/profile_inspector.rs:87-105` — Profile inspector reads `meta.in_type` / `meta.out_type` (both removed)
- `example/back/src/agent.rs:160-188` — Agent describe reads `meta.in_type` / `meta.out_type` (both removed)

### Outbound Dependencies (what the manifest depends on)

- `crate/odyssey/src/core/identity/ids.rs:68-70` — `PluginId { name, version }` (unchanged; consumer of the `plugin` field)
- `crate/odyssey/src/core/identity/kind.rs` — `CapKind` enum (unchanged; consumer of `CapabilityDecl.kind`)
- `crate/odyssey/src/core/meta/meta.rs:1-50` — `CapabilityMeta` struct (drop 2 fields at `:48-49`)
- `serde` + `serde_json` — `#[derive(Serialize, Deserialize)]` on `PluginManifest` and `CapabilityDecl` (kept; `serde_json::Value` on `tool_schema` is the only JSON coupling)

### Infrastructure Wiring

- `crate/odyssey/src/core/mod.rs:22` — `pub mod manifest;` (kept; provides the module path)
- `crate/odyssey/src/core/mod.rs:36-40` — `pub use manifest::manifest::{...}` (3 tokens dropped; affects every Rust file that imports the curated names)
- `example/back/Cargo.toml` — `odyssey` workspace dependency (no change)
- `example/fore/package.json` — TS build dependencies (no change; `tsc` will catch the type errors)

## Architecture Insights

1. **Identity-preserving 6-7 hop cascade is structural, not semantic**: every hop in the `in_type` / `out_type` propagation uses `clone()` or `as_str()`. The values never diverge — `"any"` at the builder is `"any"` at the JSON wire and `"any"` at the React render. This is what makes the cascade safe to break in one pass.

2. **Three independent JSON wire contracts share the same source fields**: `CapInfo` (HTTP `/api/caps`), `AgentCore::describe` JSON (HTTP `agent_describe`), `ProfileInspectorResource::inspect` JSON (HTTP `profile_inspect`). Each builds its `serde_json::json!({...})` literal independently — no shared serializer trait. The duplication is intentional (each contract has different optional fields like `live` / `operations` / `plugin` shape), but it means each contract must be edited in lockstep with the source fields.

3. **Hand-written TS types are the missing-fields regression net**: the doc comment at `types.tsx:1-9` explicitly states the design choice ("the point of having these as hand-written types (not generated) is to make the data binding *narrow*"). Without them, a Rust-side field removal would silently leave an unused `cap.in_type` reference in JSX; with them, the Rust deletion propagates to `tsc` errors that block `pnpm build`.

4. **`#[serde(default)]` on both fields is the lenience that allowed the dead-via-builder pattern to persist**: a TOML manifest omitting `in_type` deserializes to `in_type: ""`, which the builder immediately overwrites with `"any"`. There was no compile error or test failure that would have surfaced "this field is always set to the same value" as a defect. The FRD's Decision 5 (strict serde, parse-error on stale fields) is the structural fix to prevent recurrence.

5. **The "dead-via-builder" pattern is its own diagnostic**: every field that the builder can only set to one hardcoded value is a candidate for deletion. The four examples in this subtraction (`isolate` → `InProc`, `in_type` / `out_type` → `"any"`, `HostServiceRef.scope` → `None`) all followed this pattern.

6. **`ResourceHints` is the canonical single-field newtype anti-pattern**: it wraps exactly one field, has no `Display` / `Debug` / `From` impl, and adds one indirection at every read site (`m.resources.timeout_ms` instead of `m.timeout_ms`). The Phase 8 commit `9723503` already shrank it to a single field; this subtraction completes the job by inlining that field.

7. **Single-variant enums pay the type-system cost without providing any value**: `IsolationMode { InProc }` is a tagged enum with serde `#[serde(tag = "kind", rename_all = "snake_case")]` — the TOML/JSON shape is `{ "kind": "in_proc" }`, but the variant is constant. The enum carries zero information.

8. **The `..Default::default()` filler in `build()` is a clippy `needless_update` candidate the moment any field is added or removed** (per `0b91cb9` Phase 9 CI). Today's `build()` body at `manifest.rs:351-368` lists every field explicitly with no filler, so this lint will not fire — but after the inlined-`timeout_ms` refactor lands, the struct literal in `build()` must stay explicit (no filler) to preserve clippy-clean status.

## Precedents & Lessons

### Precedent: Phase 9 `.action`/`.protocol` scrub (`a42573f`)

**Commit(s)**: `a42573f` — "phase9/chore/scrub-write-only-metadata-dead-code" (2026-09-15)
**Blast radius**: 20+ files across 5 layers (manifest, meta, mint, capability-handle, capability-enforce, capability-mod, lifecycle_event, core-mod, builtin x3)

- `src/core/manifest/manifest.rs` — deleted `.action()` / `.protocol()` setters + the TOML loader (`from_toml_str`, `from_path`, `validate`, `ManifestError`)
- `src/core/meta/meta.rs` — deleted `Protocol` struct + 6 builders, `AuthorityContract`, `CapabilityAction`, `with_action`, `operation_for`
- `src/personality/lifecycle/mint.rs` — deleted `authority` / `protocol` propagation in `meta_from_decl`
- `src/core/mod.rs` — pruned dead re-exports
- `builtins/src/{echo,reverse,database}.rs` — deleted `.action(...)` calls

**Follow-up fixes**:

- `c22ddee` (2026-09-15) — "phase9.5/chore/drop-unused-thiserror-and-toml-deps": Phase 9 left `thiserror` and `toml` Cargo deps dangling because the deleted TOML loader / `ManifestError` were the only users. Same trap the upcoming subtraction must NOT repeat: deleting the Rust surface does not delete the dependency wiring. For the upcoming subtraction, no Cargo deps become orphans (the field deletions don't touch any `use` path), but `serde_json` is still needed for the kept `tool_schema: Option<Value>`.
- `bd3961c` (2026-09-15) — "phase9.5/docs/phase-9-changelog-section-and-cordis-trim": backfilled a `Phase 9` CHANGELOG entry that the multi-commit Phase 9 pass had silently omitted. Lesson: a multi-commit phase that shrinks the surface leaves no single "the change happened here" commit for reviewers; CHANGELOG must be updated in the same commit or a follow-up.
- `4b500e3` (2026-09-15) — "phase9/docs/fix-changelog-and-readme-lies": CHANGELOG claimed the `snapshot_index()` helper had been deleted (it had not); forward-pointing doc referenced `personality/glue/cordis_error.rs` that never shipped. Lesson: any commit that says "Phase X cleanup deletes Y" in the docstring must actually delete Y in the same commit, or the docstring becomes a future lie that the next phase has to correct.

**Lessons from docs**:

- The FRD's Pre-resolution 4 references the Phase 8 commit `9723503` as the precedent for "拍平" (flatten) `ResourceHints`. Phase 8 commit `9723503` already shrank `ResourceHints` to one field (`timeout_ms`); the FRD finishes the job by inlining that field onto `PluginManifest` itself.

**Takeaway**: A multi-struct manifest subtraction leaves orphaned Cargo deps, orphaned doc claims, and a stale CHANGELOG — all four must be hunted in the same pass.

### Precedent: Phase 8 ResourceHints slim (`9723503`)

**Commit(s)**: `9723503` — "phase8/chore/slim-resource-hints-to-timeout-only" (2026-09-15)
**Blast radius**: 1 file, `src/core/manifest/manifest.rs` (+9/-3)

- Dropped `cpu`, `mem_mb`, `io_bps` from `ResourceHints`; only `timeout_ms` remains.

**Follow-up fixes**:

- `0b91cb9` (2026-09-15) — "phase9/ci/fix-six-clippy-warnings": the partially-deleted `ResourceHints` triggered a clippy `needless_update` at `manifest.rs:296` (the `..Default::default()` next to the now-singular `timeout_ms` field). Today's `build()` body lists every `PluginManifest` field explicitly with no `..Default::default()`, so the lint will not fire after the inlined `timeout_ms` refactor lands — but the struct literal must stay explicit (no filler).

**Takeaway**: Sibling precedents always produce at least one follow-up to clean up the now-empty wrapper struct, the now-redundant `Default::default()` filler, or the warning helper. The FRD's flattening (Pre-resolution 4) avoids the follow-up by deleting the wrapper in one commit.

### Precedent: Phase 9.5 resolver dead payload (`00d1249`)

**Commit(s)**: `00d1249` — "phase9.5/refactor/drop-resolver-dead-payload" (2026-09-15)
**Blast radius**: 2 files (`src/personality/composition/resolve.rs`, no others), 1 layer (composition)

- Deleted `ContractEntry::manifest` field (carried `#[allow(dead_code)]`), `by_plugin` BTreeMap, the `let _ = by_plugin;` drop at the call site.

**Takeaway**: Dead fields that survive carry a *side-effect* in the form of duplicate-detection, dedup logic, or implicit invariants — confirm the side-effect dies with the field before deleting. For `PluginManifest.host`, the side-effect audit (11 write-only call sites, 0 readers) was already done by the FRD; the trap is whether any of those 11 call sites is being read by reflection or by an out-of-tree consumer (verified by the `rg "\.host\("` sweep across the entire repo returning zero matches outside `example/back/src/`).

### Precedent: Phase 9 CapKind unification (`218b161`)

**Commit(s)**: `218b161` — "phase9/refactor/unify-capability-kind-on-capkind" (2026-09-15)
**Blast radius**: 5 files across 3 layers (meta, manifest, identity/kind, mint, run)

- `src/core/meta/meta.rs` — replaced `streaming: bool` with `kind: CapKind`
- `src/core/manifest/manifest.rs` — replaced `streaming: bool` with `kind: CapKind`; renamed `.streaming(bool)` → `.kind(CapKind)`
- `src/core/identity/kind.rs` — `CapKind` gained `Serialize`, `Deserialize`, derived `Default (Sync)`
- `src/personality/lifecycle/mint.rs` — `meta_from_decl` copies `decl.kind` directly
- `src/personality/lifecycle/run.rs` — mint dispatch reads `decl.kind` directly

**Follow-up fixes**:

- `b32a1c4` (2026-09-15) — "phase9/test/syn-based-layering-walker-covers-builtins": added a syn-based layering test that asserted the `kind` rename had propagated to every builtin. Lesson: when a struct field on `CapabilityDecl` is renamed (or deleted, as in the upcoming subtraction), the syn-based walker is the regression net; the upcoming subtraction must run the walker (or `rg`) post-edit to confirm zero stragglers in the 11 builtin call sites + the 2 smoke test literals.

**Takeaway**: A field that gets a rename cascades through 4 distinct sites (struct field, builder literal, mint copy, reader). A field that gets deleted cascades through 5 (add the `..Default::default()` in the `build()` body, since the previous `ResourceHints { timeout_ms }` literal at `manifest.rs:366` must become `PluginManifest { ..., timeout_ms }`).

### Precedent: Phase 7 DependencyRef deletion (`5dc29e9`)

**Commit(s)**: `5dc29e9` — "phase7/feat/delete-orphan-dependencyref" (2026-09-15)
**Blast radius**: 4 files (CHANGELOG, `host/manifest/mod.rs`, `host/manifest/types.rs`, `host/mod.rs`), 1 layer (host-manifest)

- No follow-up fix — the rare "clean deletion, no follow-up" precedent because `DependencyRef` was a *type-level orphan*: no field carried it, no caller referenced it.

**Takeaway**: Single-commit type deletion is faster than the two-phase split (delete the field/builder in one phase, delete the type in the next) but skips the consumer-warning step; verify there are zero re-export paths outside `example/back/` before merging. The companion commit `a5c069c` (Phase 6 `drop-consumes-field`) notes: "`DependencyRef` itself stays public (other crates may still import it; removal is a separate concern)." The upcoming subtraction does NOT follow the two-phase split — it deletes `HostServiceRef` and `IsolationMode` in a single commit. The `rg` sweeps across the repo confirmed no third-party consumer, so the single-commit path is safe.

### Composite Lessons

1. **Re-grep Cargo.toml after every Rust-surface deletion.** `c22ddee` (Phase 9.5) proves the `a42573f` (Phase 9) deletion left `toml` and `thiserror` deps orphaned. The upcoming subtraction deletes no dependency-bearing code (no `Path` import, no `toml::` callsite), but the rule stands: any `use` path that becomes orphan must be hunted.

2. **A "we may consult it later" comment is a lie that survives.** `00d1249` (Phase 9.5) explicitly calls out the "we may consult it in a later phase" comment that justified `by_plugin` for an unknown period before deletion. The FRD's `PluginManifest.host` field has an identical surface — the only "consumer" anyone can imagine is a future host-service registry that the dispatcher would need to introspect. **The trap**: deletion forces the future registry to re-introduce both `HostServiceRef` and the host declaration path in one commit, which is more work than the original design — but it is the right cost (one-time) vs. carrying a write-only field indefinitely.

3. **Delete the writer, the reader, and the warnings in one commit.** Phase 7's `02cc1c2` (delete D5 warning helper) lagged Phase 5's quota-field deletion by two phases. The upcoming FRD's Decision 5 (strict serde, parse-error on stale fields) avoids the warning helper entirely — but it does NOT avoid the need to scrub doc comments that reference `host`, `in_type`, `out_type`, `isolate`, `IsolationMode`, `ResourceHints`, `HostServiceRef` (FR19). The doc-scrub is the trap that is most often deferred to a `phase{N+0.5}/docs/style-nits-from-review` commit; deferring it is acceptable but flag the commit's status as "code done, doc pending" so the next reviewer doesn't think the change is unfinished.

4. **`..Default::default()` filler in `build()` is a clippy `needless_update` candidate the moment any field is added or removed.** `0b91cb9` (Phase 9 CI) caught this exact lint at `manifest.rs:296` after `ResourceHints` shrank to one field. Today's `build()` body at `manifest.rs:351-368` lists every `PluginManifest` field explicitly with no `..Default::default()`, so this lint will not fire — but after the inlined-`timeout_ms` refactor lands, the struct literal in `build()` must stay explicit (no filler) to preserve clippy-clean status. `cargo clippy -- -D warnings` should be in the verification gate.

5. **The 11 builtin call sites for `.host("dispatcher")` are the highest-blast-radius single deletion in the FRD.** Phase 8's `9723503` deleted `cpu`/`mem_mb`/`io_bps` in one file; Phase 9's `a42573f` deleted `.action()` calls across three builtins. The upcoming `.host()` deletion touches 11 files (including 2 in `agent.rs`). A `rg '\.host\(' example/back/src/` before the manifest change is the only reliable enumeration — the FRD already enumerates all 11 sites, which is the right discipline.

6. **The `meta_from_decl` cascade is a 1:1 mirror of the manifest field set.** `mint.rs:48-61` copies `decl.contract_name`, `decl.kind`, `decl.tool_schema` into `CapabilityMeta`. Today it also copies `decl.in_type` and `decl.out_type` (lines 56-57). After the subtraction, `CapabilityMeta` must lose its `in_type: String` and `out_type: String` fields too (FR8, file `meta.rs:48-49`).

7. **The smoke-test literals at `example/back/tests/smoke.rs:352-353, 503-504` are test-only re-declarations that must be updated.** Phase 11's `b60fd36` (chore/rustfmt-tests-smoke) set the precedent that smoke tests carry their own `CapabilityDecl` literals independent of the builder. The two literals are exactly that pattern.

8. **Doc-comment strategy must distinguish "scrub" from "add new prose".** The FRD's Decision 7 ("only clean up dead references") matches Phase 9's pattern (`a42573f` rewrites dead doc comments in the same commit, but adds zero new prose; `cc516c5` is the same pattern one commit later). The trap: a reviewer will ask "why is `PluginManifest` documented as having a `host` field that doesn't exist?" — the answer must be that the comment was scrubbed, not that the field is `#[allow(missing_docs)]`.

9. **The CHANGELOG entry for this subtraction is mandatory, not optional.** `bd3961c` (Phase 9.5) had to backfill a Phase 9 entry that the multi-commit pass had silently omitted. The upcoming subtraction removes 4 fields (`isolate`, `host`, `resources`, plus `in_type`/`out_type` on `CapabilityDecl`), 1 enum, 2 structs, 1 builder method, and updates 5 files in `example/back/` + 1 frontend file + 1 TS types file. The CHANGELOG entry must enumerate each removal explicitly so the next reader doesn't have to walk the diff to learn what disappeared.

## Historical Context (from `.rpiv/artifacts/`)

- `.rpiv/artifacts/discover/2026-09-16_21-27-12_manifest-field-subtraction.md` — the FRD (7 locked decisions, 19 functional requirements, 0 open questions) this research validates

## Developer Context

**Q (discover: Pre-resolution 1 — `PluginManifest.host` and `HostServiceRef`):** `PluginManifest.host` is write-only (11 `.host("dispatcher")` call sites across `example/back/src/`, 0 readers anywhere in `crate/odyssey/src` or `example/back/src`). What scope of deletion?
A: Full delete of `PluginManifest.host` + `HostServiceRef` struct + `ManifestBuilder.host()` + all 11 call sites.

**Q (discover: Pre-resolution 2 — `CapabilityDecl.in_type` and `out_type`):** `in_type` and `out_type` are `dead-via-builder` but have transitive readers through `CapabilityMeta` (mint at `mint.rs:56,57`, HTTP bridge at `serve.rs:190,191`, profile inspector at `profile_inspector.rs:95,96`, agent describe at `agent.rs:182,183`, frontend card at `CapDetail.tsx:113,114`). What scope of deletion?
A: Full delete + cascade. Remove from `CapabilityDecl`, `CapabilityMeta`, the HTTP payload, the profile inspector JSON, the agent describe JSON, the frontend card.

**Q (discover: Pre-resolution 3 — `PluginManifest.isolate` and `IsolationMode`):** `PluginManifest.isolate: IsolationMode` is a single-variant enum; the only reader of the variant is the deferred `docs/deferred/` WASM/cdylib design (which has not shipped). What scope of deletion?
A: Full delete. `IsolationMode` and the `isolate` field are gone; re-introduce the enum when the WASM/cdylib loaders ship.

**Q (discover: Pre-resolution 4 — `ResourceHints` flattening):** `ResourceHints` wraps only `timeout_ms: Option<u32>`. Flatten or keep the newtype as a forward-compat slot?
A: Flatten. Inline `timeout_ms: Option<u32>` directly on `PluginManifest`; delete the `ResourceHints` struct.

**Q (discover: Decision 5 — TOML backwards compatibility):** How strict should the schema be?
A: Strict — retain `#[serde(default)]` on the remaining `Vec` / `Option` fields. Any existing TOML file referencing `host = [...]` or `in_type = "..."` would fail to parse, which is acceptable given no TOML is in production use.

**Q (discover: Decision 6 — Verification gate):** What commands must pass after the subtraction to consider the change done?
A: Full build+test — `cargo build -p odyssey && cargo test -p odyssey && cargo test -p back`.

**Q (discover: Decision 7 — Doc comment strategy):** How should doc comments be updated to reflect the new shape?
A: Only clean up dead references. Strip references to `host services`, `IsolationMode`, `ResourceHints`, `HostServiceRef`, `in_type`, `out_type` from existing doc comments; do not author new explanatory prose in this change.

**Q (`core/mod.rs:36-40`, `meta.rs:48-49`, `types.tsx:24-25,50-51`, `smoke.rs:352-353,503-504`):** The FRD enumerates a cascade list that omits 3 file-level sites — `meta.rs:48-49` (CapabilityMeta fields), `types.tsx:24-25, 50-51` (TS interfaces), and the 2 `CapabilityDecl` struct literals in `smoke.rs:352-353, 503-504`. All are silent build breaks that the FRD's gate won't catch without explicit extension. How should these be handled?
A: All three added to the research doc's file scope (developer confirmed during checkpoint).

**Q (`:build()` after `ResourceHints` flatten — `manifest.rs:351-368`):** Should the post-subtraction `build()` body use a `..Default::default()` filler for `PluginManifest`, or list every field explicitly?
A: List every field explicitly. The `..Default::default()` pattern would be a clippy `needless_update` candidate once any field is added or removed (per `0b91cb9` Phase 9 CI). Today's explicit listing stays.

## Related Research

None. This is the first research artefact for the `manifest-field-subtraction` topic.

## Open Questions

None. All checkpoint ambiguities resolved.
