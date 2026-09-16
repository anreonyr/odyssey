---
date: 2026-09-16T23:34:15+0800
author: anreonyr
commit: 3c8b0dc
branch: master
repository: odyssey
topic: "Manifest Field Subtraction"
tags: [plan, manifest, plugin, subtraction, refactor]
status: ready
parent: ".rpiv/artifacts/designs/2026-09-16_22-52-14_manifest-field-subtraction.md"
phase_count: 1
phases:
  - n: 1
    title: "Manifest Field Subtraction — single commit"
    files:
      - "crate/odyssey/src/core/manifest/manifest.rs"
      - "crate/odyssey/src/core/mod.rs"
      - "crate/odyssey/src/core/meta/meta.rs"
      - "crate/odyssey/src/personality/lifecycle/mint.rs"
      - "crate/odyssey/src/personality/lifecycle/serve.rs"
      - "crate/odyssey/src/personality/lifecycle/run.rs"
      - "example/back/src/echo.rs"
      - "example/back/src/reverse.rs"
      - "example/back/src/database.rs"
      - "example/back/src/streaming_echo.rs"
      - "example/back/src/llm.rs"
      - "example/back/src/memory.rs"
      - "example/back/src/tool_descriptor.rs"
      - "example/back/src/profile_inspector.rs"
      - "example/back/src/agent_runtime.rs"
      - "example/back/src/agent.rs"
      - "example/back/tests/smoke.rs"
      - "example/fore/src/api/types.tsx"
      - "example/fore/src/pages/CapDetail.tsx"
      - "CHANGELOG.md"
      - "README.md"
    depends_on: []
last_updated: 2026-09-16T23:34:15+0800
last_updated_by: anreonyr
---

# Manifest Field Subtraction Implementation Plan

## Overview

A pure deletion refactor across 21 files in 5 layers (kernel, personality, example backend, frontend, top-level docs). Removes every dead or dead-via-builder field on `PluginManifest` and its nested types, deletes the supporting types (`IsolationMode`, `HostServiceRef`, `ResourceHints`), and cascades the deletion through every reader. Single commit on `phase12/chore/scrub-…-host-service-vocabulary` following Phase 9 `a42573f` precedent. Reference design: `.rpiv/artifacts/designs/2026-09-16_22-52-14_manifest-field-subtraction.md`.

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

## What We're NOT Doing

- `requires.kind` / `optional` / `VersionRange` — needs a resolver consumer; deferred per research Composite Lesson 6.
- `IsolationMode` Wasm/Subprocess variants — deferred until WASM/cdylib loaders ship.
- `Permissions` block (`can_spawn_subprocess`, `can_read_fs`, `can_open_network`) — deferred until a host sandbox exists.
- Strongly-typed `tool_schema` — deferred until a second consumer beyond `tool_descriptor` reads it.
- `docs/deferred/` changes — out of scope per design Decision 7.
- Any semantic change to capability resolution, minting, or runtime — out of scope.
- Any change to `PluginId` or `CapKind` — kept as-is.

## Phase 1: Manifest Field Subtraction — single commit

### Overview

Single-commit refactor that removes 4 fields (`PluginManifest.isolate`/`host`/`resources`; `CapabilityDecl.in_type`/`out_type`), 1 enum (`IsolationMode`), 2 structs (`HostServiceRef`, `ResourceHints`), 1 builder method (`ManifestBuilder.host()`), and cascades the deletion through 8 reader sites (mint, serve `CapInfo`, serve `list_caps`, run, `profile_inspector`, `agent_describe`, `types.tsx` ×2 interfaces, `CapDetail.tsx`) — 16 individual field/key accesses — across 21 files. Includes the module-doc, field/enum/struct doc, defaults-table, historical-narrative, and `expose`-method-doc scrub inside `manifest.rs`, 1 new `CHANGELOG.md` entry, 1 `README.md` line update. Final shape: `PluginManifest` collapses from 6 fields to 4 (`plugin`, `exposes`, `requires`, `timeout_ms`).

### Changes Required

#### 1. Target data shape

**File**: `crate/odyssey/src/core/manifest/manifest.rs`
**Changes**: Target file. Drops 3 fields from `PluginManifest` (`isolate`, `host`, `resources`), drops 1 enum (`IsolationMode`), drops 2 structs (`HostServiceRef`, `ResourceHints`), drops 1 builder method (`host()`), drops 4 `in_type`/`out_type` literals from 4 `expose*` methods, scrubs the module doc, field/enum/struct docs, the `ManifestBuilder` defaults-table rows, the historical-narrative paragraph, and the `expose` method doc paragraph. New `build()` constructs 4-field `PluginManifest` with every field listed explicitly (no `..Default::default()` filler, per Phase 9 CI `0b91cb9` clippy precedent).

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

#### 2. Re-export collapse

**File**: `crate/odyssey/src/core/mod.rs`
**Changes**: Drops 3 tokens (`HostServiceRef`, `IsolationMode`, `ResourceHints`) from `pub use manifest::manifest::{...}` re-export block at `:39-40`; drops 3 names from module doc at `:17-18`. Stale `pub use` import would fail `cargo build -p odyssey`.

```rust
//! - `manifest` — `PluginManifest`, `ManifestBuilder`,
//!   `CapabilityDecl`, `CapabilityRequirement`.

// Curated re-exports — the public surface of core.
pub use manifest::manifest::{
    CapabilityDecl, CapabilityRequirement, ManifestBuilder, PluginManifest,
};
```

#### 3. CapabilityMeta mirror

**File**: `crate/odyssey/src/core/meta/meta.rs`
**Changes**: Drops 2 fields (`in_type`, `out_type`) from `CapabilityMeta` struct. Mirror of `CapabilityDecl`.

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

#### 4. Mint cascade

**File**: `crate/odyssey/src/personality/lifecycle/mint.rs`
**Changes**: Drops 2 clone writes (`in_type: decl.in_type.clone()`, `out_type: decl.out_type.clone()`) in `meta_from_decl` at lines 56-57. Compiles against the new 9-field `CapabilityMeta`.

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

#### 5. HTTP bridge

**File**: `crate/odyssey/src/personality/lifecycle/serve.rs`
**Changes**: Drops 2 fields (`in_type: String`, `out_type: String`) from `CapInfo` struct at lines 73-74; drops 2 clone writes in `list_caps` at lines 190-191. New 4-field `CapInfo` matches TS `CapInfo` interface.

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

#### 6. Orchestrator

**File**: `crate/odyssey/src/personality/lifecycle/run.rs`
**Changes**: One-token rename at line 233 — `entry.manifest.resources.timeout_ms` → `entry.manifest.timeout_ms` (after `ResourceHints` flatten).

```rust
        for decl in &entry.manifest.exposes {
            let budget = CapabilityBudget::new(entry.manifest.timeout_ms.unwrap_or(5000));
            let slot_id = (entry.mint_fn)(factory, plugin_id, decl, decl.kind, budget, bindings);
```

#### 7. echo builtin

**File**: `example/back/src/echo.rs`
**Changes**: Drops `.host("dispatcher")` chain call at line 58 in `EchoBuiltin::manifest()`.

```rust
        ManifestBuilder::new("echo")
            .expose_with_schema("echo", "echo", tool_schema)
            .timeout_ms(5000)
            .build()
```

#### 8. reverse builtin

**File**: `example/back/src/reverse.rs`
**Changes**: Drops `.host("dispatcher")` chain call at line 71 in `ReverseBuiltin::manifest()`.

```rust
        ManifestBuilder::new("reverse")
            .expose_with_schema("reverse", "reverse", tool_schema)
            .timeout_ms(5000)
            .build()
```

#### 9. database builtin

**File**: `example/back/src/database.rs`
**Changes**: Drops `.host("dispatcher")` chain call at line 165 in `DatabaseBuiltin::manifest()`.

```rust
        ManifestBuilder::new("database")
            .expose_with_schema("database", "database", tool_schema)
            .timeout_ms(5000)
            .build()
```

#### 10. streaming_echo builtin

**File**: `example/back/src/streaming_echo.rs`
**Changes**: Drops `.host("dispatcher")` chain call at line 144 in `StreamingEchoBuiltin::manifest()`.

```rust
        ManifestBuilder::new("streaming_echo")
            .expose_streaming_with_schema("streaming_echo", "streaming_echo", tool_schema)
            .timeout_ms(5000)
            .build()
```

#### 11. llm builtin

**File**: `example/back/src/llm.rs`
**Changes**: Drops `.host("dispatcher")` chain call at line 1095 in `LlmBuiltin::manifest()`.

```rust
        ManifestBuilder::new("llm")
            .expose(NAME_COMPLETE, CONTRACT_COMPLETE)
            .expose(NAME_EMBED, CONTRACT_EMBED)
            .timeout_ms(30000)
            .build()
```

#### 12. memory builtin

**File**: `example/back/src/memory.rs`
**Changes**: Drops `.host("dispatcher")` chain call at line 506 in `MemoryBuiltin::manifest()`.

```rust
        ManifestBuilder::new("memory")
            .expose(NAME_QUERY, CONTRACT_QUERY)
            .expose(NAME_INSERT, CONTRACT_INSERT)
            .timeout_ms(5000)
            .build()
```

#### 13. tool_descriptor builtin

**File**: `example/back/src/tool_descriptor.rs`
**Changes**: Drops `.host("dispatcher")` chain call at line 169 in `ToolDescriptorBuiltin::manifest()`.

```rust
        ManifestBuilder::new("tool_descriptor")
            .expose(NAME, CONTRACT)
            .timeout_ms(5000)
            .build()
```

#### 14. profile_inspector builtin

**File**: `example/back/src/profile_inspector.rs`
**Changes**: Drops `.host("dispatcher")` chain call at line 149 in `ProfileInspectorBuiltin::manifest()`; drops 2 JSON keys (`"in_type": meta.in_type`, `"out_type": meta.out_type`) in `ProfileInspectorResource::inspect` at lines 95-96.

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

#### 15. agent_runtime builtin

**File**: `example/back/src/agent_runtime.rs`
**Changes**: Drops `.host("dispatcher")` chain call at line 1548 in `AgentRuntimeBuiltin::manifest()`.

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

#### 16. agent builtins (list + describe)

**File**: `example/back/src/agent.rs`
**Changes**: Drops 2 `.host("dispatcher")` chain calls at lines 295 and 340 in `AgentListBuiltin::manifest()` and `AgentDescribeBuiltin::manifest()`; drops 2 JSON keys (`"in_type": meta.in_type`, `"out_type": meta.out_type`) in `AgentCore::describe` at lines 182-183.

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

#### 17. test struct literals

**File**: `example/back/tests/smoke.rs`
**Changes**: Drops 2 lines (`in_type: "any".into()`, `out_type: "any".into()`) from 2 `CapabilityDecl` struct literals (lines 350-357 in `agent_reaches_only_its_bindings_and_reports_revocation`, lines 501-508 in `agent_describe_refuses_unknown_fields`). Literals stay explicit (no `..Default::default()` filler).

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

#### 18. TS interfaces

**File**: `example/fore/src/api/types.tsx`
**Changes**: Drops `in_type: string` / `out_type: string` from `CapInfo` interface (lines 24-25) and from `AgentHandleDescribeLive` interface (lines 50-51). Hand-written TS types are the missing-fields regression net; without them a Rust-side removal would silently leave an unused `cap.in_type` reference in JSX.

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

#### 19. CapDetail JSX

**File**: `example/fore/src/pages/CapDetail.tsx`
**Changes**: Drops 2 `<Meta>` JSX rows (`<Meta label="in" value={cap.in_type} mono />`, `<Meta label="out" value={cap.out_type} mono />`) at lines 113-114. Surrounding `<Separator />` at line 112 stays.

```tsx
              <Meta label="id" value={cap.id} mono />
              <Meta label="plugin" value={capToPlugin(cap.name)} mono />
              <Meta label="timeout" value={`${cap.timeout_ms} ms`} mono />
              <Separator />
```

#### 20. CHANGELOG entry

**File**: `CHANGELOG.md`
**Changes**: Add new `### Changed — Phase 12: manifest field subtraction cascade` entry at top of `[Unreleased]` section, before the existing `### Added — oxlint + oxfmt…` entry at line 9. Style: first-person plural, bold bullet leads, double-backticked code identifiers, no `file:line` references in bullets (per Keep-a-Changelog convention; prior entries do not cite line numbers).

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

#### 21. README example

**File**: `README.md`
**Changes**: Drops `"in_type":"any","out_type":"any",` from the `agent_describe` JSON example at line 356 in the `## Agent builtin` section. The `streaming` field stays (derived from kept `kind`).

```
"streaming":false,
```

### Success Criteria

#### Automated Verification

- [x] Type checking passes: `cargo build -p odyssey`
- [x] Tests pass: `cargo test -p odyssey`
- [x] Tests pass: `cargo test -p back`
- [x] No `.host(` calls in kernel: `rg '\.host\(' crate/odyssey/src` returns 0 matches
- [x] No `in_type`/`out_type` in Rust: `rg 'in_type|out_type' crate/odyssey/src example/back/src` returns 0 matches
- [x] No `IsolationMode`/`HostServiceRef`/`ResourceHints` in kernel: `rg 'IsolationMode|HostServiceRef|ResourceHints' crate/odyssey/src` returns 0 matches
- [x] No `manifest.resources` access: `rg 'manifest\.resources' crate/odyssey/src` returns 0 matches
- [x] No remaining `.host(` in example backend: `rg '\.host\(' example/back/src` returns 0 matches
- [x] Frontend types compile: `pnpm --dir example/fore build` exits 0 (or its equivalent)
- [x] 1:1 builder↔data-shape invariant: every `PluginManifest` field is settable via the builder, every builder method sets a field that exists

#### Manual Verification

- [x] Frontend `CapDetail` card no longer shows `in: any` / `out: any` rows
- [x] `/api/caps` JSON response no longer contains `in_type` / `out_type` keys
- [x] `agent_describe` JSON response no longer contains `in_type` / `out_type` keys
- [x] `profile_inspect` JSON response no longer contains `in_type` / `out_type` keys
- [x] `CHANGELOG.md` `[Unreleased]` section has the new `### Changed — Phase 12: …` entry
- [x] `docs/deferred/` TOML sketches are unchanged (no scrub)
- [x] `CHANGELOG.md` historical entries at lines 210, 626, 649, 864, 867, 869 are unchanged (no scrub)

---

## Testing Strategy

### Automated

- `cargo build -p odyssey` exits 0 (catches unused `pub use` re-exports at `core/mod.rs:36-40` and struct field deletions at `manifest.rs:38-58, 106-146`).
- `cargo test -p odyssey` exits 0 (resolver + orchestrator + mint tests).
- `cargo test -p back` exits 0 (24 integration tests in `smoke.rs`).
- `pnpm --dir example/fore build` exits 0 (TS type contract at `types.tsx:24-25, 47-48`).

### Manual Testing Steps

1. Run `rg '\.host\(' crate/odyssey/src example/back/src` — must return 0 matches.
2. Run `rg 'in_type|out_type' crate/odyssey/src example/back/src` — must return 0 matches.
3. Run `rg 'IsolationMode|HostServiceRef|ResourceHints' crate/odyssey/src` — must return 0 matches.
4. Run `rg 'manifest\.resources' crate/odyssey/src` — must return 0 matches.
5. Verify `docs/deferred/` (5 files: 2 readmes, 2 TOMLs, 1 WAT) is unchanged.
6. Verify `CHANGELOG.md` historical entries at lines 210, 626, 649, 864, 867, 869 are unchanged.

## Performance Considerations

No specific impact. Pure deletion refactor that removes dead code and indirection. Post-subtraction `CapabilityMeta` is 9 fields (was 11); the per-call budget lookup is a one-token rename (no algorithmic change). HTTP bridge emits smaller JSON payloads (2 fewer string fields per capability in 3 wire contracts) — minor reduction in wire bytes for high-cardinality `/api/caps` responses.

## Migration Notes

- **TOML manifests with old fields fail to parse**: any TOML file referencing `host = [...]`, `in_type = "..."`, `out_type = "..."`, or `[isolate] kind = "in_proc"` will fail to deserialize under the strict serde config (design Decision 5). No TOML is in production use today, so the breakage is theoretical.
- **CHANGELOG entry is mandatory** (research Composite Lesson 9): a multi-commit pass that leaves a CHANGELOG silence is a reviewer-facing regression. The `### Changed — Phase 12: …` entry must be in the same commit as the code changes.
- **No Cargo dependency follow-up**: the subtraction does not orphan any `use` path. `serde_json` survives because `tool_schema: Option<Value>` is the only JSON coupling remaining.

## Developer Context

Phase 1 is the only phase. Single-commit refactor: kernel → personality → backend → frontend → tests → docs all land in `phase12/chore/scrub-manifest-dead-fields-and-host-service-vocabulary`. Step 4 reviewer findings (artifact-code-reviewer + artifact-coverage-reviewer) land in the `## Plan Review (Step 4)` section appended at the end of this artifact.

## References

- Design: `.rpiv/artifacts/designs/2026-09-16_22-52-14_manifest-field-subtraction.md`
- Research: `.rpiv/artifacts/research/2026-09-16_22-07-52_manifest-field-subtraction.md`
- Discover (FRD): `.rpiv/artifacts/discover/2026-09-16_21-27-12_manifest-field-subtraction.md`

## Plan Review (Step 4)

_Independent post-finalization review by artifact-code-reviewer and artifact-coverage-reviewer subagents. Findings triaged at Step 5. Tally: 0 blockers, 0 concerns, 8 suggestions._

| source | plan-loc | codebase-loc | severity | dimension | finding | recommendation | resolution |
| --- | --- | --- | --- | --- | --- | --- | --- |
| code | Phase 1 §5 (serve.rs) | `crate/odyssey/src/personality/lifecycle/serve.rs:73-74` | suggestion | codebase-fit | Plan claims `CapInfo` `in_type`/`out_type` fields are at lines 75-76, but they are at lines 73-74 (off by 2). Code is correct, only the citation is wrong. | Update fence description to cite lines 73-74. | applied: line refs corrected to 73-74 / 190-191 |
| code | Phase 1 §5 (serve.rs) | `crate/odyssey/src/personality/lifecycle/serve.rs:190-191` | suggestion | codebase-fit | Plan claims `list_caps` clones are at lines 195-196, but they are at lines 190-191 (off by 5). Code is correct, only the citation is wrong. | Update fence description to cite lines 190-191. | applied: line refs corrected to 73-74 / 190-191 |
| code | Phase 1 §6 (run.rs) | `crate/odyssey/src/personality/lifecycle/run.rs:233` | suggestion | codebase-fit | Plan claims the one-token rename is at line 230, but `entry.manifest.resources.timeout_ms` is at line 233 (off by 3). The code change itself is correct; only the citation is wrong. | Update fence description to cite line 233. | applied: line ref corrected to 233 |
| code | Phase 1 §2 (core/mod.rs) | `crate/odyssey/src/core/mod.rs:17-18, 39-40` | suggestion | codebase-fit | Plan cites the module-doc span as `:14-19` and the re-export block as `:36-40`; actual lines are `:17-18` (doc) and `:39-40` (re-export). Code is correct, only the citations are wrong. | Update fence descriptions to cite the actual `:17-18` and `:39-40` line ranges. | applied: line ranges corrected to 17-18 / 39-40 |
| code | Phase 1 §16 (agent.rs) | `example/back/src/agent.rs:182-183` | suggestion | codebase-fit | Plan claims the `in_type`/`out_type` JSON keys in `AgentCore::describe` are at lines 185-186; actual lines are 182-183 (off by 3). The two `.host()` calls at 295 and 340 are correctly cited. Code is correct, only the citation is wrong. | Update fence description to cite lines 182-183 for the JSON keys. | applied: line ref corrected to 182-183 |
| code | Phase 1 §18 (types.tsx) | `example/fore/src/api/types.tsx:50-51` | suggestion | codebase-fit | Plan claims `AgentHandleDescribeLive`'s `in_type`/`out_type` fields are at lines 47-48; actual lines are 50-51 (off by 3). The `CapInfo` lines 24-25 are correctly cited. Code is correct, only the citation is wrong. | Update fence description to cite lines 50-51. | applied: line ref corrected to 50-51 |
| code | Phase 1 §3 (meta.rs) | `crate/odyssey/src/core/meta/meta.rs` | suggestion | code-quality | Performance Considerations + Plan §4 prose claim `CapabilityMeta` "is 11 fields (was 13)" — actual is 9 fields post-subtraction (was 11). Both before and after counts are off by 2. | Correct the count claim to "9 fields (was 11)". | applied: count corrected to 9 fields (was 11) |
| code | Phase 1 §1 (manifest.rs) | `crate/odyssey/src/core/manifest/manifest.rs` | suggestion | code-quality | Plan claims "scrubs 18 doc-comment blocks inside `manifest.rs`"; actual count of distinct scrubbed blocks is roughly 10-12 depending on how table rows are counted. The 18 figure is propagated from the research/FRD and is the intended target but the actual visible scrub depends on how you count. | Soften the count to drop the precise 18, or describe what's scrubbed (module doc, field/enum/struct docs, defaults table rows, historical paragraph, expose doc). | applied: 18 replaced with enumerated scrubbed sections |
| code | Phase 1 (Overview) | <n/a> | suggestion | code-quality | Overview claims "cascades the deletion through 16 readers across 21 files"; the actual distinct reader sites are 8 (mint, serve CapInfo, serve list_caps, run, profile_inspector, agent describe, types.tsx×2, CapDetail.tsx) — 16 is correct only if counting each field access separately, which the prose doesn't clarify. | Either rephrase as "8 reader sites" or explicitly say "16 individual field accesses across 8 sites". | applied: rephrased to 8 reader sites / 16 field accesses |
| coverage | <n/a> | <n/a> | <n/a> | verification-coverage | _No findings — coverage reviewer cleared the artifact. Walked every Verification Notes / Migration Notes / Pattern References entry from the design; all are covered by a phase Success Criterion, a visible code mirror, or a Testing Strategy / Migration Notes item in the plan._ | <n/a> | <n/a> |
