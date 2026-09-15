# odyssey

A seL4-style capability kernel implemented in Rust. Three strictly
layered modules — `core` (value types + abstract traits), `capability`
(kernel implementation), `personality` (orchestration) — cooperate via
trait objects and a workspace-member `builtins/` crate that supplies
typed capability handlers. No plugins are compiled into the library;
the example binary at `examples/basic.rs` wires the three retained
builtins (echo / reverse / database) into the orchestrator and serves
the HTTP bridge on `127.0.0.1:3030`.

## Possession model

```
CapabilitySpace                ← seL4 CSpace (kernel owns)
└── SlotId                     ← stable position in the space
    └── Slot<R>                ← typed, unforgeable reference (unit of possession)
        └── Capability<R>      ← what occupies the slot
            ├── meta: CapabilityMeta       (id, name, namespace, contract)
            ├── budget: Arc<CapabilityBudget>  (per-call wall-clock cap)
            ├── clock: Arc<dyn Clock>      (kernel-side time source)
            └── handler: Arc<R>             (the resource)
```

Builtins never look up capabilities by string name at runtime. They hold
`Slot<R>` references minted at activation time and dispatch through
the typed capability.

## seL4 mapping

| seL4                          | odyssey                                                       |
| ----------------------------- | ------------------------------------------------------------- |
| `CNode.Allocate`              | `CapabilityFactory::mint::<R>(...)`                          |
| CNode capability (handle)     | `Capability<R>` (`Arc`)                                       |
| CNode slot                    | `SlotId` + `Slot<R>` reference                                |
| CSpace                        | `CapabilitySpace`                                             |
| `endpoint.send`               | `token.invoke()` / `token.open()` / `slot.invoke()`           |
| Resource badge                | `CapabilityBudget.timeout_ms` (per-call wall clock)           |
| Slot revocation               | `cspace.revoke(slot_id)` — slot ref stays valid, lookup fails |

## Type axes

- `R` (resource type) — what the builtin declares
  (`EchoResource`, `ReverseResource`, `DatabaseResource`, ...).
  Sync vs Stream is distinguished at runtime via `CapKind`,
  not at the type level. The `Slot<R>` reference is uniform;
  `slot.invoke(...)` works on sync caps; `slot.open(...)` works
  on streaming caps; calling the wrong one returns
  `CapabilityError::KindMismatch`.

```rust
// Builtins construct typed slots from the SlotId returned by
// their inherent mint() method. The plugin's plugin code
// stores the slot in a struct field; dispatch is just
// `self.slot.invoke(input)`.
let slot: Slot<EchoResource> = Slot::new(cspace.clone(), slot_id);

slot.invoke(json!({"hello": "world"}))?;     // sync — wall-clock budget enforced
// For streaming caps:
// let stream: Slot<GeneratorResource> = Slot::new(cspace.clone(), stream_slot_id);
// stream.open(json!("hi"))?;                  // returns Receiver<CapabilityChunk>
```

## Three-layer architecture

Phase 8 split the codebase into three strictly-layered modules:

```
personality ──▶ capability ──▶ core
                │            │
                └────────────┘
```

- `core` — value types (`CapabilityId`, `SlotId`, `PluginId`,
  `CapabilityMeta`, `OperationRights`, `QuotaSpec`, ...) + abstract
  traits (`Resource`, `BuiltinManifest`). Pure leaf layer. No I/O,
  no `Instant::now()` direct calls (use `clock::Clock`).

- `capability` — kernel implementation: `Capability<R>`, `Slot<R>`,
  `AnyCapability`, `CapabilitySpace`, `QuotaState`,
  `CapabilityBudget`, `CapabilityEventBus`. Depends only on `core`.

- `personality` — orchestration: manifest resolution,
  lifecycle (mint / ruin / serve), HTTP bridge,
  `LifecycleEventBus`. Depends only on `core` and `capability`.

The library has no plugins. Workspace-member `builtins/` provides
the typed plugin handlers; the example binary
`examples/basic.rs` wires them in.

A `tests/layering.rs` integration test reads the source at every
test run and asserts the dependency direction.

## Boot order

`personality::lifecycle::run` is the top-level orchestrator:

1. Setup     — load manifests from builtins; create cspace + factory.
2. Resolve   — compute mint order + binding tables from manifests.
3. Mint      — install typed `Capability<R>` per (plugin, exposes).
4. Serve     — HTTP bridge on `127.0.0.1:3030`; wait for Ctrl-C.
5. Teardown  — `ruin_runtime_plugins` in reverse mint order.

Phase 5 had an 8-phase orchestrator with a no-op `Phase 5`
placeholder. Phase 8 collapses to the 5 functional phases above.

## HTTP bridge

```
GET  /api/caps    →  enumerate occupied slots (CapabilityMeta snapshot)
POST /api/invoke  →  invoke a sync capability by name
POST /api/stream  →  open a streaming capability by name (SSE)
```

The HTML UI is at `examples/frontend/index.html`; `serve.rs` embeds
it via `include_str!`.

## Layout

```
odyssey/
├── Cargo.toml                # workspace = [".", "builtins"]
├── CHANGELOG.md
├── README.md
├── src/
│   ├── lib.rs                # 3 modules: capability, core, personality
│   ├── capability/           # kernel implementation
│   │   ├── mod.rs
│   │   ├── enforce/          # quota runtime state + cspace
│   │   │   ├── mod.rs
│   │   │   ├── quota.rs      # QuotaState + CapabilityBudget (state)
│   │   │   └── space.rs      # CapabilitySpace + CapabilityEventBus +
│   │   │                      # CapabilityEvent + DeriveKind
│   │   ├── handle/           # typed + erased capability handles
│   │   │   ├── mod.rs
│   │   │   ├── slot.rs       # Slot<R>
│   │   │   └── cap/
│   │   │       ├── mod.rs
│   │   │       ├── typed.rs  # Capability<R>
│   │   │       └── erased.rs # AnyCapability
│   │   └── error.rs          # CapabilityError (typed variants)
│   ├── core/                 # shared value types + abstract traits
│   │   ├── mod.rs
│   │   ├── identity/{mod,ids,kind}.rs
│   │   ├── clock/{mod,clock}.rs
│   │   ├── meta/{mod,meta,chunk}.rs
│   │   ├── manifest/{mod,manifest}.rs
│   │   ├── rights/{mod,rights}.rs
│   │   ├── quota/{mod,quota}.rs     # value types
│   │   └── contract/{mod,builtin,resource}.rs
│   └── personality/          # orchestration
│       ├── mod.rs
│       ├── composition/{mod,resolve}.rs
│       └── lifecycle/
│           ├── mod.rs
│           ├── boot.rs              # load_manifests + print_manifests
│           ├── lifecycle_event.rs   # LifecycleEvent + LifecycleEventBus
│           ├── mint.rs              # CapabilityFactory + meta_from_decl
│           ├── ruin.rs              # ruin_runtime_plugins
│           ├── run.rs               # run() orchestrator (the entry point)
│           └── serve.rs             # axum router + spawn_http_bridge
├── examples/
│   ├── basic.rs              # wires builtins → personality orchestrator
│   └── frontend/index.html   # HTTP bridge UI
├── tests/
│   ├── layering.rs           # asserts core ⊥ capability ⊥ personality
│   └── smoke.rs              # echo builtin mint + typed-slot + invoke round-trip
└── builtins/                 # workspace member
    ├── Cargo.toml
    └── src/
        ├── lib.rs
        ├── echo.rs
        ├── reverse.rs
        └── database.rs
```

## Build

```sh
cargo build --workspace                  # library + builtins
cargo build --workspace --examples       # library + builtins + examples/basic.rs
cargo test                              # integration tests: layering (3) + smoke (1)
cargo run --example basic               # boot the orchestrator + HTTP bridge
```

The example binary listens on `127.0.0.1:3030` until Ctrl-C.

## Builtin contract

Each builtin in `builtins/src/<name>.rs` exports:

```rust
pub struct <Name>Resource;                       // the resource type

impl Resource for <Name>Resource { ... }          // Resource::invoke + Resource::open

pub struct <Name>Builtin;                         // the builtin wrapper

impl BuiltinManifest for <Name>Builtin {          // core::contract::builtin
    fn manifest(&self) -> PluginManifest { ... }
}

impl <Name>Builtin {
    pub fn mint(
        &self,
        factory: &CapabilityFactory,
        plugin: &PluginId,
        decl: &CapabilityDecl,
        kind: CapKind,
        budget: CapabilityBudget,
    ) -> Result<SlotId, String> {
        // build Arc<<Name>Resource>, call factory.mint(kind, decl, plugin, budget, handler)
    }
}

// Each builtin also implements the personality-side Mint trait
// (the typed mint dispatch surface). The trait lives in
// personality::lifecycle::run and is the second of the two
// traits a builtin must implement.
impl Mint for <Name>Builtin {
    fn mint(...) -> Result<SlotId, String> {
        <Name>Builtin::mint(self, ...)
    }
}
```

The example binary at `examples/basic.rs` wires the three builtins:

```rust
use std::sync::Arc;
use odyssey::personality::lifecycle::run::run;
use odyssey_builtins::{database, echo, reverse};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    run(
        Arc::new(echo::EchoBuiltin),
        Arc::new(reverse::ReverseBuiltin),
        Arc::new(database::DatabaseBuiltin),
    )
    .await
}
```

`run` boots the kernel, collects manifests from the three
builtins, resolves the dependency graph, mints each capability
into the cspace via the builtin's typed `mint()` method, brings
up the HTTP bridge, waits for Ctrl-C, and tears down in reverse
mint order.

## Test

`cargo test` runs `tests/layering.rs`, which reads every
`src/**/*.rs` file and asserts:

- `src/core/**` has no `use crate::capability` or
  `use crate::personality` imports.
- `src/capability/**` has no `use crate::personality` imports.
- `src/personality/**` uses `crate::capability` (the typed mint
  dispatch lands here, not in `core`).

These invariants catch accidental layer crossings during future
refactors.
