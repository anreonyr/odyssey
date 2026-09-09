# Changelog

All notable changes to odyssey are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/), and this project
adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
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
