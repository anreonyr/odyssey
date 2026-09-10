# odyssey

A seL4-style capability kernel implemented in Rust. Plugins are manifest-
driven: each plugin module declares a typed resource and exposes a
capability; the host mints unforgeable `Capability<R, K>` tokens into a
`CapabilitySpace`, installs them at fresh `SlotId` positions, and hands
plugin bodies the typed `Slot<R, K>` reference. The host can revoke a
slot's contents without invalidating the plugin's slot reference — the
plugin's `invoke` / `open` / `capability` then fails closed.

## Possession model

```
CapabilitySpace                ← seL4 CSpace (host owns)
└── SlotId                     ← stable position in the space
    └── Slot<R, K>             ← typed, unforgeable reference (unit of possession)
        └── Capability<R, K>   ← what occupies the slot
            ├── meta: CapabilityMeta       (id, name, types, timeout_ms)
            ├── budget: Arc<CapabilityBudget>  (per-call wall-clock cap)
            └── handler: Arc<R>             (the resource)
```

Plugins never look up capabilities by string name at runtime. They hold
`Slot<R, K>` references minted at activation time and dispatch through
the typed capability.

## seL4 mapping

| seL4                          | odyssey                                                       |
| ----------------------------- | ------------------------------------------------------------- |
| `CNode.Allocate`              | `CapabilityFactory::mint_sync` / `mint_stream`                |
| CNode capability (handle)     | `Capability<R, K>` (`Arc`)                                    |
| CNode slot                    | `SlotId` + `Slot<R, K>` reference                             |
| CSpace                        | `CapabilitySpace`                                             |
| `endpoint.send`               | `token.invoke()` / `token.open()` / `slot.invoke()`           |
| Resource badge                | `CapabilityBudget.timeout_ms` (per-call wall clock)           |
| Slot revocation               | `cspace.revoke(slot_id)` — slot ref stays valid, lookup fails |

## Type axes

- `R` (resource type) — what the plugin module declared
  (`EchoResource`, `ReverseResource`, `GeneratorResource`, ...).
- `K` (kind) — `SyncKind` or `StreamKind`, encoded via `PhantomData`
  so the type system distinguishes sync vs streaming capabilities.

```rust
let slot: Slot<EchoResource, SyncKind>        = ctx.require("slot:echo")?;
let stream: Slot<GeneratorResource, StreamKind> = ctx.require("slot:generate")?;

slot.invoke(json!({"hello": "world"}))?;     // sync — wall-clock budget enforced
stream.open(json!("hi"))?;                   // returns Receiver<CapabilityChunk>
```

## Boot order

1. Parse manifests
2. Provide core services (`capability_space`, `capability_factory`, `registry`)
3. Mint typed tokens, allocate slots, install + provide slots to cordis
4. **Fail-fast** dependency check — abort boot if any `consumes` is missing
5. Start plugin fibers — cordis resolves `slot:<name>` inject declarations
6. Demo harness — invoke via typed slots
7. HTTP bridge on `127.0.0.1:3030`

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
│   ├── lifecycle.rs     — 7-phase orchestrator (boot sequence)
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

plugins/                 — one TOML manifest per plugin
├── agent.toml
├── database.toml
├── echo.toml
├── echo_chain.toml
├── echo_stream.toml
├── embedder.toml
├── generator.toml
├── http.toml
├── reverse.toml
├── sandbox.toml
└── slow.toml

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

Each plugin module in `src/plugins/<name>.rs` exports:

```rust
pub struct <Name>Resource;                          // the resource type

impl SyncResource for <Name>Resource { ... }        // or StreamResource

pub fn handler() -> Arc<<Name>Resource>;           // the handler factory
pub fn <name>_plugin() -> Arc<dyn Plugin>;          // the cordis plugin fiber
```

The host reads `plugins/<name>.toml`, mints a typed token wrapping the
plugin's handler, allocates a fresh slot, installs the token there, and
provides a `Slot<R, K>` reference to cordis under the key `slot:<name>`.
The plugin's body injects this slot via:

```rust
plugin_with("<name>", vec![Injection::from("slot:<name>")], |ctx, _| async move {
    let slot: Arc<Slot<<Name>Resource, SyncKind>> = ctx.require("slot:<name>")?;
    slot.invoke(json!({...}))?;  // or slot.open(...) for streams
    Ok(())
})
```