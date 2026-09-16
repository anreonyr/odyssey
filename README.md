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
  `slot.invoke(op, ...)` works on sync caps; `slot.open(...)` works
  on streaming caps; calling the wrong one returns
  `CapabilityError::KindMismatch`. The `op` is `OperationRights`
  (a bitflag of `READ | WRITE | EXECUTE | ADMIN`); the kernel
  checks the held rights against the requested op and returns
  `CapabilityError::OperationDenied` on a miss. See
  `example/back/tests/smoke.rs::attenuated_capability_denies_unheld_op`
  for the contract.

```rust
// Builtins construct typed slots from the SlotId returned by
// their inherent mint() method. The plugin's plugin code
// stores the slot in a struct field; dispatch is just
// `self.slot.invoke(op, input)`.
let slot: Slot<EchoResource> = Slot::new(cspace.clone(), slot_id);

slot.invoke(OperationRights::EXECUTE, json!({"hello": "world"}))?;
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

```text
odyssey/                       # root has no Cargo.toml — only scripts and docs
├── scripts/
│   ├── back.sh                # cargo build + run the example binary
│   ├── fore.sh                # pnpm build/dev/preview/test for the React app
│   └── test.sh                # cargo test in both crates + the jsdom smoke
├── crate/                     # workspace A — the library
│   ├── Cargo.toml             # members = ["odyssey"]
│   └── odyssey/
│       ├── Cargo.toml
│       ├── src/                # lib.rs + 3 modules: capability, core, personality
│       │   ├── lib.rs
│       │   ├── capability/
│       │   ├── core/
│       │   └── personality/
│       └── tests/
│           └── layering.rs     # asserts core ⊥ capability ⊥ personality
└── example/                   # not a workspace — just a directory
    ├── back/                  # the example binary + the built-in plugins
    │   ├── Cargo.toml         # single crate: [package] + [lib] + [[bin]]
    │   ├── src/
    │   │   ├── lib.rs          # builtin module root (lib name `odyssey_builtin`)
    │   │   ├── main.rs         # binary entry — wires builtins → orchestrator
    │   │   ├── agent.rs       # read-only observers (list, describe)
    │   │   ├── agent_runtime.rs
    │   │   ├── echo.rs
    │   │   ├── reverse.rs
    │   │   ├── database.rs
    │   │   ├── streaming_echo.rs
    │   │   ├── llm.rs
    │   │   ├── memory.rs
    │   │   ├── tool_descriptor.rs
    │   │   └── profile_inspector.rs
    │   └── tests/
    │       └── smoke.rs        # builtin round-trips + frontend data-binding
    └── fore/                  # Vite + React + TS — agent UI
        ├── package.json
        ├── pnpm-lock.yaml
        ├── pnpm-workspace.yaml
        ├── .oxlintrc.json     # oxlint config — correctness + react/import/vitest/jsx-a11y plugins
        ├── .oxfmtrc.json      # oxfmt config — Prettier defaults + sortImports + sortTailwindcss
        ├── tsconfig.json
        ├── vite.config.tsx    # Vite config; --config vite.config.tsx is passed because Vite 5 doesn't auto-discover .tsx
        ├── tailwind.config.tsx # Tailwind config (loaded via direct import, not require'd path)
        ├── index.html         # Vite entry — restores after restructure
        ├── src/
        ├── test/frontend.test.tsx  # jsdom smoke test, run via `pnpm exec tsx`
        └── dist/              # gitignored, built on demand
```

`crate/` is the only cargo workspace (lib only). `example/back/` is a
single crate holding both the builtin **lib** (`odyssey_builtin`) and the
example **bin** (`odyssey-example-back`); the previous `example/` workspace

- `backend/odyssey-builtin/` member + their two `Cargo.toml`s collapsed
into one. The example imports the lib across workspaces via
`path = "../../crate/odyssey"`. The root has no `Cargo.toml` and no
`pnpm-lock.yaml` — pnpm only serves `example/fore/`.

## Build

```sh
cd crate         && cargo build           # lib
cd example/back  && cargo build           # example binary
cd crate         && cargo test            # layering (3)
cd example/back  && cargo test            # smoke (27)

./scripts/back.sh                       # build + run the example binary
./scripts/fore.sh build                 # type-check + bundle the React app
./scripts/fore.sh lint                  # oxlint (read-only)
./scripts/fore.sh lint:fix              # oxlint --fix
./scripts/fore.sh format                # oxfmt (write)
./scripts/fore.sh format:check          # oxfmt --check (CI)
./scripts/test.sh                       # all of the above + the jsdom smoke

# Set ODYSSEY_ADDR=host:port to move the bridge (the smoke test
# asks the OS for a free port and passes it through, so its
# assertions always run against the build under test rather
# than whatever else is listening).
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
     "streaming":false,
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
`example/fore/dist/index.html` into jsdom via
`example/fore/test/frontend.test.tsx`, drives it, and
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
