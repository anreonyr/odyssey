# odyssey

A seL4-style capability kernel implemented in Rust. Three strictly
layered modules — `core` (value types + abstract traits), `capability`
(kernel implementation), `personality` (orchestration) — cooperate via
the `AnyCapability` erased view and a workspace-member `builtins/`
crate that supplies typed capability handlers. No plugins are
compiled into the library; the example binary at `examples/basic.rs`
wires eleven builtins (echo / reverse / database / streaming_echo /
agent_list / agent_describe / tool_descriptor / profile_inspector /
llm / memory / agent_runtime) into the orchestrator and serves the
HTTP bridge on `127.0.0.1:3030`.

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

The agent frontend is a Vite + React build at
`examples/frontend/`; `serve.rs` reads the resulting
`dist/index.html` from disk at request time. The library stays
UI-free (no `include_str!`, no embed-time binding). The frontend
covers the registered capabilities grouped by plugin and the
agent's session lifecycle: start a session, drive it step by
step with Tick / ToolResult / UserReply / Stream, inspect and
edit the kernel's memory.

The first two are wired to each other. `/api/caps` supplies the
capability list; `agent_list` supplies which of them the agent can
reach, drawn dimmed when the answer is none. The agent table is one
`agent_describe` per reachable handle, which is what puts the
`operations` column on the page: `CapabilityMeta` carries no
rights, so `/api/caps` structurally cannot report them —
`AnyCapability::operations()` can, and only through the agent. The
two agent capabilities themselves appear dimmed, because neither
appears in a `requires`: they are absent from the reachable set for
the same reason as any other capability, which is that the set is
exactly what the binding table names.

Reaching itself is not merely undeclared, it is refused: the
resolver answers `ResolveError::SelfRequirement` for a manifest
whose `requires` resolves back to its own capabilities. No mint
order satisfies it, since the plugin would have to be minted before
it could bind to itself.

If the agent is not mounted the page degrades rather than breaks:
the capability panel and both invoke panels keep working, and the
agent panel says so instead of showing an empty table. That path is
covered by the test described under Test below.

`POST /api/stream` takes `{"capability": "<name>", "input": <json>}`
and answers with an SSE stream of `chunk` events followed by one
`done` event. `streaming_echo` (the only streaming builtin today)
reads `{"text": "<string>", "count": N}` and an optional
`delay_ms`; the delay exists so the browser can display chunks
progressively rather than as one TCP packet's worth of simultaneous
output.

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
│   └── frontend/            # Vite + React + TS — agent UI
├── tests/
│   ├── layering.rs           # asserts core ⊥ capability ⊥ personality
│   └── smoke.rs              # builtin round-trips + agent binding-table behaviour + frontend data-binding
└── builtins/                 # workspace member
    ├── Cargo.toml
    └── src/
        ├── lib.rs
        ├── agent.rs              # read-only observers (list, describe)
        ├── agent_runtime.rs      # AI agent runtime (start/resume/cancel/plan/stream/...)
        ├── echo.rs
        ├── reverse.rs
        ├── database.rs
        ├── streaming_echo.rs
        ├── llm.rs                # LLM provider plugin (mock for MVP)
        ├── memory.rs             # in-process memory backend
        ├── tool_descriptor.rs    # reads a cap's `tool_schema`
        └── profile_inspector.rs  # reads a cap's full CapabilityMeta
```

## Build

```sh
cargo build --workspace                  # library + builtins
cargo build --workspace --examples       # library + builtins + examples/basic.rs
cargo test                              # integration tests: layering (3) + smoke (27)
cargo run --example basic               # boot the orchestrator + HTTP bridge

# The agent frontend lives in `examples/frontend/` and is a
# Vite + React build. `cargo run --example basic` reads the
# resulting `dist/index.html` from disk, so the React bundle is a
# precondition for the example — not for `cargo build`, but for
# any `cargo run --example basic` or `cargo test` that needs the
# page rendered.
pnpm --dir examples/frontend install
pnpm --dir examples/frontend build
```

The example binary listens on `127.0.0.1:3030` until Ctrl-C; set
`ODYSSEY_ADDR=host:port` to move it. The variable exists for tests:
the frontend smoke test asks the OS for a free port and passes it
through, so its assertions always run against the build under test
rather than whatever else is listening.

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
    ) -> SlotId {
        // build Arc<<Name>Resource>, call factory.mint(kind, decl, plugin, budget, handler)
    }
}

// `register()` is the single entry point the orchestrator
// consumes: it publishes the manifest and the two dispatch
// function pointers together, so a builtin cannot ship one
// half without the other. Phase 10 replaced the per-plugin
// `Mint` marker trait with the `MintFn` function pointer
// below (the trait was pure forwarding boilerplate); Phase 11
// added the symmetric `RuinFn`. No builtin ships a custom
// teardown yet — every `RuinFn` is `default_ruin`.
pub type MintFn = fn(
    factory: &CapabilityFactory,
    plugin: &PluginId,
    decl: &CapabilityDecl,
    kind: CapKind,
    budget: CapabilityBudget,
    bindings: &[ResolvedBinding],
) -> SlotId;

pub type RuinFn = fn(cspace: &CapabilitySpace, slot_ids: &[SlotId]) -> Result<usize, String>;

impl <Name>Builtin {
    pub fn register() -> (PluginManifest, MintFn, RuinFn) {
        (
            <Name>Builtin.manifest(),
            |factory, plugin, decl, kind, budget, bindings| {
                <Name>Builtin.mint(factory, plugin, decl, kind, budget, bindings)
            },
            default_ruin,
        )
    }
}
```

`bindings` is the minting plugin's own row of
`ResolvedPlan::bindings` — the capabilities its `requires`
resolved to. It is empty for a plugin that declares none, which
is every builtin except the agent. The orchestrator injects it
at mint time because minting walks `plan.mint_order`, so a
consumer is always minted after the providers it binds to.
`RuinFn` returns the number of slots it revoked: the hook owns
the revoke, so the orchestrator reports its count instead of
revoking a second time to derive one.

The example binary at `examples/basic.rs` builds the registry from
those helpers:

```rust
use odyssey::personality::lifecycle::run::run;
use odyssey_builtins::{agent, database, echo, reverse, streaming_echo};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let plugins = vec![
        echo::EchoBuiltin::register(),
        reverse::ReverseBuiltin::register(),
        database::DatabaseBuiltin::register(),
        streaming_echo::StreamingEchoBuiltin::register(),
        agent::AgentListBuiltin::register(),
        agent::AgentDescribeBuiltin::register(),
    ];
    run(&plugins).await
}
```

`run` boots the kernel, collects manifests from the six
builtins, resolves the dependency graph — the agent's `requires`
edges are the only ones, so it is ordered after its four
providers — mints each capability into the cspace via the
registry's typed `MintFn`, brings up the HTTP bridge, waits for
Ctrl-C, and tears down in reverse mint order.

## Agent builtin

`builtins/src/agent.rs` exposes two read-only capabilities over
the reachable set its `requires` resolved to:

```
POST /api/invoke  {"capability":"agent_list","input":{}}
  → {"handles":[{"handle":"echo","live":true}, ...]}

POST /api/invoke  {"capability":"agent_describe","input":{"handle":"echo"}}
  → {"handle":"echo","live":true,"capability":"echo","contract":"echo",
     "name":"echo","namespace":"echo","plugin":"echo","kind":"sync",
     "streaming":false,"in_type":"any","out_type":"any",
     "timeout_ms":5000,"calls_per_minute":0,
     "operations":["READ","WRITE","EXECUTE","ADMIN"]}
```

Two properties hold by construction rather than by convention:

- **The reachable set is the binding table.** Every name the
  agent resolves comes out of a `ResolvedBinding`. A capability
  installed in the cspace under a name no `requires` mentioned
  is unreachable: `describe` answers `agent: unknown handle`
  for it, because `reach` searches the table before it touches
  the cspace.
- **Nothing is snapshotted.** `CapabilityMeta` carries no
  operation rights — those live on `Capability<R>` behind
  `AnyCapability::operations()`. So the agent stores
  declarations and resolves live per call. A capability revoked
  after mint reports `live: false` with its capability-level
  fields absent, rather than a stale record claiming it exists.

The agent observes and never invokes. Erased invocation
(`AnyCapability::invoke_dyn`) does not consult `OperationRights`
the way the typed `invoke_op` does — the HTTP bridge has the
same gap — so an agent that dispatched would inherit it. Closing
that gap is separate work.

## Test

`cargo test` runs two integration binaries: `tests/layering.rs` (3
tests) and `tests/smoke.rs` (8 tests).

`tests/layering.rs` parses every `.rs` file with `syn`, walks every
`UseTree`, and asserts:

- `src/core/**` has no `use crate::capability` or
  `use crate::personality` imports.
- `src/capability/**` has no `use crate::personality` imports.
- `src/personality/**` uses `crate::capability` (the typed mint
  dispatch lands here, not in `core`).

It also scans the `builtins/` workspace member, and handles group
imports (`use crate::{capability, personality};`), aliased imports,
and nested groups — the earlier line-scanner missed all three.

`tests/smoke.rs` covers the behavioural side: an echo mint + typed
slot + invoke round-trip, a `streaming_echo` `open` round-trip that
collects every chunk, the sync-invoke-on-a-streaming-cap
`KindMismatch` rejection, a self-requiring manifest resolving to
`ResolveError::SelfRequirement` (and a mutual pair still resolving to
`Cycle`), `agent_describe` refusing an unknown field, and two agent
tests — one resolving the agent's manifests end to end and asserting
the binding row and mint order they produce, one driving `AgentCore`
directly to pin the three reachability outcomes (unreachable,
revoked, unbound).

The frontend test, `frontend_script_binds_to_the_live_api`, is the
only one that spans both halves: it spawns the example binary,
loads the **built** React bundle from
`examples/frontend/dist/index.html` into jsdom via
`examples/frontend/test/frontend.test.mjs`, drives it, and
asserts the rendered DOM — that every plugin / capability row
renders, that the four reachable handles surface all four rights,
that caps outside the agent's binding row are marked, that the
agent section's Run / Timeline / Memory panels are present. It
exists because `curl` and reading the HTML each prove only one
side: neither catches a field renamed on one side while the other
keeps looking for the old name, which renders as a missing column
rather than an error. jsdom cannot catch layout or CSS breakage,
only data-binding breakage.

It runs the child on a port it asks the OS for, and reaps the child
before asserting. Both are deliberate: with a fixed port the test
either fights whatever holds it, or connects to that server and
passes while judging the wrong build — which is what happened before
the port became a parameter. It skips itself when `node` is absent.

These invariants catch accidental layer crossings during future
refactors.
