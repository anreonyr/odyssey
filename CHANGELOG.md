# Changelog

All notable changes to odyssey are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/), and this project
adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added — Phase 2: Capability-Native Composition

Phase 2 builds on Phase 1's authority model and asks: **can a system
where every authority-bearing component is a Capability-bearing Plugin
form a complete computation model?** The completion criteria are six
properties P1–P6, with P4 (Channel), P5 (Same Program Different
Authority), and P6 (Capability Graph) being new in Phase 2.

- **Quota system** (`src/capability/types.rs`): `CapabilityBudget`
  gains `QuotaState { calls_per_minute, tokens_per_minute,
  bytes_per_minute }` enforced via sliding-window timestamps.
  `Capability::invoke_op` rejects with `QuotaExceeded` after the
  limit. `CapabilityError::QuotaExceeded { name, kind }` added.
- **Typed contracts** (`src/capability/types.rs`):
  `CapabilityContract { input_schema, output_schema, description }`
  carried inside `CapabilityMeta`. Surfaced via the HTTP bridge
  in a later Phase.
- **Hierarchical namespacing** (`src/capability/cspace.rs`):
  `CapabilityMeta.namespace`, `enumerate_namespace(prefix)`,
  `namespace_children(prefix)`. Names like `odyssey.model.llama3`
  are first-class.
- **Channel plugin** (`src/plugins/channel/`): producer/consumer
  pair wrapped as `Capability<ChannelResource>` and
  `Capability<ConsumerResource>`. P4 — capability-controlled
  communication; revoke(channel) severs the send path.
- **Capability Graph** (`src/capability/graph.rs`): `CapabilityGraph`
  exposes the namespace tree + parent→child relationships via
  `children_of(slot)`. P6 — composition is observable.
- **RuleAgent plugin** (`src/plugins/agent/`): type-agnostic
  orchestrator that introspects capabilities via `AnyCapability::
  operations()` and dispatches via `invoke_op_dyn`. P5 — same
  program, different capabilities → different reachable world.
- **Multi-hop revocation** (`src/capability/cspace.rs`):
  `cspace.revoke_tree(slot)` severs every descendant slot.
  Combined with the new `parents` map in `CSpaceInner`, this makes
  revoking an intermediate link in a Broker A → Broker B chain
  kill every downstream cap.
- **AnyCapability introspection** (`src/capability/cap.rs`):
  `operations()` and `invoke_op_dyn(op, input)` added so erased
  callers (Agent, Pipeline) can read and exercise per-operation
  authority without a typed `R`.
- **`Slot<R>::invoke_op` and `revoke_tree`** — convenience on the
  typed slot reference.
- **Six new labs** (`src/lab/`):
  `quota`, `namespace`, `graph`, `channel`, `agent`, `multi_hop`.
  Wired through `cargo run --bin lab -- <name>`.
- **Seven new property tests** (`tests/capability_phase2.rs`):
  `p3_revoke_tree_severs_descendants`, `p4_capability_channel_severs_on_revoke`,
  `p5_same_agent_different_caps_different_behavior`,
  `p6_graph_exposes_attenuation_tree`,
  `quota_blocks_after_per_minute_limit`,
  `p2_multihop_attenuation_holds_at_each_hop`,
  `contract_survives_mint`. All pass.

### Added — Phase 1: Capability lab

Phase 1 is the **capability experiment phase**: prove the model itself
is real before building Agent / LLM / Embedder on top of it.

- **`OperationRights` bitflags** (`src/capability/types.rs`):
  `READ | WRITE | EXECUTE | ADMIN`. `Capability<R>` now carries the
  full rights bag; `Capability::invoke_op(op, input)` is the kernel-
  level guard that consults the bits at every call.
- **Real attenuation in `restrict` / `grant` / `transfer`**: all three
  now verify `rights(child) ⊆ rights(parent)` and return
  `CapabilityError::AttenuationViolation` when violated. This is the
  invariant the previous `restrict()` was missing.
- **`CapabilityRights` extended** with `operations: OperationRights`
  alongside `timeout_ms`. Helpers: `contains`, `intersect`, `root`.
- **`CounterResource` plugin** (`src/plugins/counter/`): the keystone
  experimental resource. One shared integer behind a `Mutex`, three
  operations (`read`/`increment`/`reset`) gated by
  `READ`/`WRITE`/`ADMIN` respectively.
- **`BrokerResource` plugin** (`src/plugins/broker/`): the first
  inter-plugin delegation. Holds a `Capability<CounterResource>` and
  mints derived slots via `cspace.restrict` on `{"op":"delegate"}`.
- **`SyncStage::from_slot(slot_id, cspace)`** (`src/host/pipeline.rs`):
  slot-bound pipeline stage. Re-resolves on every `run`, so revocation
  propagates without touching the pipeline. Existing
  `SyncStage::new(cap)` is preserved as `Pinned` for the legacy demo.
- **`PipelineError::SlotRevoked`** for the slot-bound failure path.
- **`bin/lab.rs`** — `cargo run --bin lab -- <authority|delegation|
  revocation|composition>` boots a fresh CSpace per experiment and
  prints a self-explanatory transcript.
- **Property tests** (`tests/capability_lab.rs`): the six properties
  the brief calls out — authority, delegation, restrict-attenuation,
  revocation, budget, composition — plus two bonus tests
  (`grant_preserves_source_capability`, `transfer_moves_and_clears_
  source`). All 8 pass.

### Added (earlier, kept here)
- **Possession model**: `CapabilitySpace` (seL4 CSpace analogue) +
  `Slot<R, K>` typed reference + `Capability<R, K>` occupant.
  Plugins hold `Slot<R, K>` references; the host can revoke slot
  contents without invalidating the reference.
- **Generic `Capability<R, K>`** with `K = SyncKind | StreamKind`
  encoded via `PhantomData`. Type system distinguishes sync and
  streaming capabilities; the type-erased `AnyCapability` trait
  bridges heterogeneous caps in the service registry.
- **Per-call wall-clock budget** enforced in `Capability::invoke`
  via `Instant::now()`; an exceeded budget returns `Err` instead
  of the handler's result.
- **Fail-fast dependency check** at boot (Phase 4): every
  manifest `consumes` must have a provider, else abort.
- **HTTP bridge** (`src/http_bridge.rs`): `GET /api/caps` enumerates
  occupied slots; `POST /api/invoke` and `POST /api/stream` route
  by capability name; SSE for streaming.
- **Plugin modules** in `src/plugins/` with typed resource structs
  (`EchoResource`, `ReverseResource`, `GeneratorResource`, ...),
  each implementing `SyncResource` or `StreamResource`.
- **Compound capability** `echo-chain` wraps the typed
  `Capability<EchoResource, SyncKind>` in its `SyncResource::invoke`.
- **WASM-isolated sandbox** with fuel budget and host-imported
  `log` function.

### Changed
- **Library split**: `src/lib.rs` exposes the shared module tree so
  `bin/odyssey` (full boot + HTTP) and `bin/lab` (Phase 1
  experiments) can share `capability`, `host`, `plugins`, and `lab`.
- **Arc-shared state**: `CapabilityService` and `CapabilityFactory`
  use `Arc<Mutex<...>>` / `Arc<AtomicU64>` so `Clone` shares inner
  state. Plugins, the service, and the HTTP bridge all see the
  same registrations.
- **`ctx.provide` correctness**: pass the inner value (`T`),
  not `Arc<T>`, to avoid the cordis `Arc<Arc<T>>` double-wrap.
- **Boot phases reduced** from 8 to 7 (services provided in phase 2;
  mint + install + provide compressed into phase 3).
- **HTTP bridge reads timeout from `CapabilityMeta.timeout_ms`**
  instead of hard-coded 5000.

### Fixed
- **Bug 1**: `CapabilityService::clone()` was creating a fresh empty
  service on every clone. Fixed by wrapping inner state in `Arc`;
  main, plugins, and the HTTP bridge now share the same registry.
- **Bug 2**: `inject` declaration required so cordis fibers wait
  for typed tokens before activating. Plugin bodies now declare
  every `slot:<name>` they depend on.
- **Bug 3**: `BudgetGuard` was creating tokens whose `Err` was
  silently dropped via `let _ = guard.check()`. Replaced with an
  inline `Instant::now()` check that returns `Err` and drops the
  handler's result on timeout.
- **Bug 4**: Plugin module abstractions were dead — main.rs held
  inline duplicate handlers. Moved every handler implementation
  into its plugin module, and main now dispatches by plugin name.
- **Ctrl-C exit**: `axum::serve` did not know about Ctrl-C, so
  `server_handle.await` blocked forever. Wired the shutdown signal
  through `axum::serve(...).with_graceful_shutdown(...)`.

### Removed
- `limiter.rs` — global `Limiter` service; superseded by per-token
  `CapabilityBudget`.
- `loader.rs` / `wasm_loader.rs` — entire file dead after plugin
  module split. `echo-cdylib` loader is reserved in
  `echo-cdylib/README.md` for future wiring.
- `SyncCap` / `StreamCap` / `SyncCapImpl` / `StreamCapImpl`,
  `CapabilityBindings`, marker structs (`EchoCap` etc.),
  `ErasedSyncHandler` / `ErasedStreamHandler`, `BudgetGuard`,
  `BudgetSnapshot`, `TypeRef` enum, `SyncStage::Pass` variant.
- `libloading` dependency (no cdylib loader wired).
- 78 compiler warnings dropped to 0.

## [0.1.0] — initial

- Initial seL4-style capability kernel with `CapabilityToken` and
  string-based dispatch.
