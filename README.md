# odyssey

A seL4-style capability kernel implemented in Rust. Plugins are manifest-
driven: each plugin module declares a typed resource and exposes a
capability; the host mints unforgeable `Capability<R>` tokens into a
`CapabilitySpace`, installs them at fresh `SlotId` positions, and hands
plugin bodies the typed `Slot<R>` reference. The host can revoke a
slot's contents without invalidating the plugin's slot reference — the
plugin's `invoke` / `open` / `capability` then fails closed.

## Possession model

```
CapabilitySpace                ← seL4 CSpace (host owns)
└── SlotId                     ← stable position in the space
    └── Slot<R>                ← typed, unforgeable reference (unit of possession)
        └── Capability<R>      ← what occupies the slot
            ├── meta: CapabilityMeta       (id, name, types, timeout_ms)
            ├── budget: Arc<CapabilityBudget>  (per-call wall-clock cap)
            ├── clock: Arc<dyn Clock>      (kernel-side time source)
            └── handler: Arc<R>             (the resource)
```

Plugins never look up capabilities by string name at runtime. They hold
`Slot<R>` references minted at activation time and dispatch through
the typed capability.

## seL4 mapping

| seL4                          | odyssey                                                       |
| ----------------------------- | ------------------------------------------------------------- |
| `CNode.Allocate`              | `CapabilityFactory::mint_sync` / `mint_stream`                |
| CNode capability (handle)     | `Capability<R>` (`Arc`)                                       |
| CNode slot                    | `SlotId` + `Slot<R>` reference                                |
| CSpace                        | `CapabilitySpace`                                             |
| `endpoint.send`               | `token.invoke()` / `token.open()` / `slot.invoke()`           |
| Resource badge                | `CapabilityBudget.timeout_ms` (per-call wall clock)           |
| Slot revocation               | `cspace.revoke(slot_id)` — slot ref stays valid, lookup fails |

## Type axes

- `R` (resource type) — what the plugin module declared
  (`EchoResource`, `ReverseResource`, `GeneratorResource`, ...).
  Sync vs Stream is distinguished at runtime via `CapKind`,
  not at the type level. The `Slot<R>` reference is uniform;
  `slot.invoke(...)` works on sync caps; `slot.open(...)` works
  on streaming caps; calling the wrong one returns
  `CapabilityError::KindMismatch`.

```rust
let slot: Slot<EchoResource> = ctx.require("slot:echo")?;

slot.invoke(json!({"hello": "world"}))?;     // sync — wall-clock budget enforced
// For streaming caps:
// let stream: Slot<GeneratorResource> = ctx.require("slot:generate")?;
// stream.open(json!("hi"))?;                  // returns Receiver<CapabilityChunk>
```

## Boot order

1. Parse manifests (11 runtime plugins + 11 `manifest()` fns)
2. Provide core services (`capability_space`, `capability_factory`)
3. Resolve capability dependency graph (`host::resolver`)
4. Mint runtime plugins in resolved order (`runtime::mint`)
5. Log legacy `consumes` entries (informational; resolver uses `requires`)
6. Activate runtime plugins in resolved order (`runtime::activate`)
7. HTTP bridge on `127.0.0.1:3030` (`runtime::http_bridge`) + wait for Ctrl-C
8. Teardown in reverse mint order (`runtime::teardown`)

## HTTP bridge

```
GET  /api/caps    →  enumerate occupied slots (CapabilityMeta snapshot)
POST /api/invoke  →  invoke a sync capability by name
POST /api/stream  →  open a streaming capability by name (SSE)
```

## Layout

Phase 5 splits the codebase into three layers (kernel / host /
runtime); plugins live in a fourth directory that depends on
the first two.

```
src/
├── lib.rs               — root module: pub mod host/kernel/plugins/runtime
├── main.rs              — entry point (boot the runtime)
├── kernel/              — pure capability kernel (no I/O, no Instant::now)
│   ├── ids.rs           — CapabilityId, SlotId, PluginId
│   ├── kind.rs          — CapKind (Sync vs Stream)
│   ├── rights.rs        — OperationRights, CapabilityRights, parse_operation
│   ├── meta.rs          — CapabilityMeta, AuthorityContract, Protocol
│   ├── chunk.rs         — CapabilityChunk (stream item enum)
│   ├── clock.rs         — Clock trait + SystemClock + MockClock
│   ├── resource.rs      — Resource trait
│   ├── error.rs         — CapabilityError (typed variants)
│   ├── slot.rs          — Slot<R> (unforgeable typed reference)
│   ├── cap/             — Capability<R> (typed) + AnyCapability (erased)
│   ├── quota/           — QuotaSpec + QuotaState + CapabilityBudget
│   └── space/           — CapabilitySpace + derivation + revocation +
│                          graph + events + namespace
├── host/                — composition: parse manifests, mint, resolve
│   ├── manifest/        — TOML loader + ManifestBuilder + types
│   ├── factory.rs       — CapabilityFactory (mint + install into CSpace)
│   ├── mint.rs          — meta_from_decl + namespace_for helpers
│   ├── resolver/        — index + topo + plan (capability-keyed resolve)
│   └── pipeline.rs      — linear composition of typed sync stages
├── runtime/             — adapter: lifecycle, HTTP bridge, plugin mint
│   ├── lifecycle.rs     — 8-phase orchestrator (boot sequence)
│   ├── activate.rs      — activator_for + per-arm const asserts
│   ├── teardown.rs      — ruin_runtime_plugins (reverse-order shutdown)
│   ├── http_bridge.rs   — axum router + SSE serve loop
│   └── mint/            — mint_runtime_plugins dispatch table
└── plugins/             — plugin bodies (depend on kernel + host)
    ├── mod.rs           — re-exports
    ├── echo/            — basic + chain + stream sub-plugins
    ├── generator/       — http-backed Markov/mock model
    ├── database/        — key-value store
    ├── embedder/        — placeholder for embedding-model integration
    ├── http/            — HTTP bridge capability
    ├── reverse/         — string reversal demo
    ├── sandbox/         — WASM-isolated execution with fuel
    ├── slow/            — exceeds its budget to exercise timeout
    ├── agent/           — generic program interpreter (Phase 4 P4.4)
    │   ├── handler.rs   — struct + constructors + parse_operation
    │   ├── dispatch.rs  — sync Resource::invoke body
    │   ├── stream.rs    — async Resource::open body + run_program
    │   ├── plugin.rs    — cordis glue (handler, handler_from_plan,
    │   │                  agent_plugin)
    │   ├── panic.rs     — panic_payload_to_str formatter
    │   ├── program.rs   — ProgramStep (per-step action spec)
    │   └── manifest.rs  — TOML descriptor
    └── test_only/       — fixtures used only by the test suite
        ├── broker/      — delegates to counter via a require handle
        ├── channel/     — streaming channel fixture
        └── counter/     — shared integer behind a Mutex

echo-cdylib/             — workspace member; cdylib loader deferred
frontend/index.html      — minimal HTTP bridge UI
tests/                   — integration test suites (alpha/beta/...)
                           cargo test --features loom-tests runs the
                           concurrency/D-test loom gate
```

## Build

```sh
cargo build
cargo run
```

## Plugin contract

Each plugin module in `src/plugins/<name>/` exports:

```rust
pub struct <Name>Resource;                          // the resource type

impl Resource for <Name>Resource { ... }             // Resource::invoke + Resource::open

pub fn handler() -> Arc<<Name>Resource>;             // the handler factory
pub fn <name>_plugin() -> Arc<dyn Plugin>;          // the cordis plugin fiber
pub fn manifest() -> &'static PluginManifest;       // the typed manifest
```

The host reads `manifest()` (a `&'static PluginManifest` returned by
each runtime plugin module), mints a typed token wrapping the plugin's
handler, allocates a fresh slot, installs the token there, and
provides a `Slot<R>` reference to cordis under the key `slot:<name>`.
The plugin's body injects this slot via:

```rust
plugin_with("<name>", vec![Injection::from("slot:<name>")], |ctx, _| async move {
    let slot: Arc<Slot<<Name>Resource>> = ctx.require("slot:<name>")?;
    slot.invoke(json!({...}))?;  // or slot.open(...) for streams
    Ok(())
})
```