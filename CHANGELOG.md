# Changelog

All notable changes to odyssey are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/), and this project
adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Breaking — Phase 8: three-layer split (core / capability / personality) + plugins out

Phase 8 is a complete restructure of the codebase. The crate is
now organised around three strictly-layered modules, with the
plugin system reduced to a workspace-member `builtins/` crate
that depends only on `core`.

- **Three-layer dependency direction.** The crate has three
  top-level modules: `core`, `capability`, `personality`. The
  direction is
  ```
      personality ──▶ capability ──▶ core
                      │            │
                      └────────────┘
  ```
  `core` is a leaf (no internal deps). `capability` depends on
  `core`. `personality` depends on `core` and `capability`.
  A `tests/layering.rs` integration test reads the source and
  asserts these invariants at every test run.

- **`src/{host,kernel,runtime,plugins}/` deleted.** The legacy
  Phase 5 three-layer split was retired: the kernel moved to
  `src/capability/`, the host decomposed into `core/manifest`
  + `personality/composition` + `personality/lifecycle`, the
  runtime moved to `personality/lifecycle/{boot, mint, ruin,
  run, serve}`. Plugins (`src/plugins/`) were deleted in favor of
  the workspace-member `builtins/` crate.

- **`builtins/` workspace member.** `echo`, `reverse`, and
  `database` are the three retained built-in capabilities.
  Each is a concrete type implementing `BuiltinManifest` (the
  only core trait for builtins — typed mint dispatch happens via
  inherent methods, not a `Builtin<R>` trait object, because
  `CapabilityFactory::mint<R>` is generic over `R` and a trait
  object can't dispatch into a generic call). The
  `examples/basic.rs` binary wires the three builtins into the
  orchestrator and serves the HTTP bridge on `127.0.0.1:3030`.

- **`src/main.rs` → `examples/basic.rs`.** The library binary
  was removed to break a cyclic dependency between the `odyssey`
  crate and the `odyssey-builtins` workspace member. The example
  binary is the only consumer of builtins; the library is
  plugin-free.

- **`Resource` trait moved to `core/contract/resource.rs`.**
  The kernel-side contract for capability handlers is now in
  `core` (where `capability` and `personality` can both depend
  on it without a cycle), not in `capability/`.

- **`BuiltinManifest` trait added in `core/contract/builtin.rs`.**
  Each builtin implements `BuiltinManifest::manifest() -> PluginManifest`.
  Typed mint happens via inherent methods on each builtin's
  concrete struct (e.g. `EchoBuiltin::mint(factory, plugin, decl, kind, budget)`).

- **`CapabilityFactory` simplified.** The redundant
  `metas: Arc<Mutex<Vec<CapabilityMeta>>>` snapshot field is gone
  (it drifted out of sync with the cspace after revoke). The
  dead `snapshots()` / `clock()` accessors are gone. The
  factory's only role is `mint()` + `space()` accessor.

- **`GraphEvent` split into `CapabilityEvent` + `LifecycleEvent`.**
  Kernel-side events (`Minted`, `Derived`, `Revoked`,
  `RevokeTree`) live in `capability::enforce::space::CapabilityEvent`.
  Personality-side events (`PluginActivated`,
  `PluginDeactivated`, `ShutdownStarted`, `ShutdownCompleted`)
  live in `personality::lifecycle::lifecycle_event::LifecycleEvent`.
  The kernel has no knowledge of plugins or shutdown; the
  personality has its own `LifecycleEventBus`.

- **`QuotaSpec` + `QuotaKind` + `QuotaSnapshot` moved to
  `core::quota`.** These are pure value types. The
  runtime-state counterparts (`QuotaState`, `CapabilityBudget`)
  stay in `capability::enforce::quota` because they hold locks
  and depend on `Clock`.

- **Dead code removed.** `kernel/space/graph.rs` (CapabilityGraph
  had zero production callers — only tests used it).
  `host/pipeline.rs` (Pipeline / SyncStage had zero production
  callers after the Phase 4 agent-driven composition pattern).
  `RuntimeQuotas::tokens_per_minute` / `bytes_per_minute` quota
  fields. `manifest.cpu` / `mem_mb` / `io_bps` ResourceHints
  fields. `CapabilityError::contains()` method (test-only
  substring predicate). `From<CapabilityError> for cordis::Error`
  (boundary leak — capability knew about cordis). `PluginManager::drop_*`,
  `RemovedQuotaFields`, and other Phase 5 dead-code-removal
  candidates.

- **`echo-cdylib/` workspace member deleted.** README explicitly
  marked it deferred. Zero production callers; rebuilt on every
  `cargo build` for no benefit.

- **`tests/` directory deleted.** 50 .rs files, ~3000 LoC. The
  Phase 8 layering invariant test that replaces them lives at
  `tests/layering.rs` (~150 LoC).

- **`Cargo.toml` cleanup.** Dropped `loom-tests` feature and
  the `loom` optional dep. Dropped `wat` + `wasmtime` (only
  the deleted sandbox plugin used them). Dropped `rand`
  (only the deleted generator plugin used it). Dropped
  `frontend/` workspace-root directory (moved to
  `examples/frontend/`).

- **Removed `manifest.cpu` / `mem_mb` / `io_bps` fields from
  `ResourceHints`.** Zero readers anywhere in `src/`. Same
  family as Phase 5 D5 (dead quota fields).

### Removed — Cordis `From` impl boundary leak

The `From<CapabilityError> for cordis::Error` impl previously
lived in `kernel/error.rs`. This was a boundary leak — the
kernel had no business knowing about `cordis`. Removed in
Phase 8; if a future binary wants to surface capability errors
via cordis, it provides its own glue in the personality layer.

### Removed — CapabilityGraph snapshot view

`kernel/space/graph.rs` (154 LoC) deleted. `CapabilityGraph` was
the only consumer-test surface for `/api/graph`-style
introspection. With tests discarded and no HTTP `/api/graph`
endpoint planned, the entire graph snapshot machinery is
gone. The `cspace.snapshot_index()` helper that backed it is
also removed; introspection use cases can re-derive their own
join on demand.

### Removed — host/pipeline.rs

Linear composition of typed sync stages (`Pipeline`,
`SyncStage`). 149 LoC. Zero production callers. The Phase 4
agent-driven composition pattern replaced it for the agent
plugin; no other consumer existed.

### Phase 6: Drop legacy `consumes`

- **Manifest `[[consumes]]` field removed.** The legacy
  plugin-version-keyed `consumes` block is gone from
  `PluginManifest`; the resolver walks `[[requires]]`
  exclusively. Migration: any `[[consumes]]` block in a
  manifest must be moved to `[[requires]]` (the contract
  name, not the plugin name, is the binding key). The only
  remaining call site, `echo_chain`, now declares its echo
  dependency via `.requires("echo", "echo")` (the legacy
  `.consumes(...)` setter is removed from `ManifestBuilder`).
- **`ManifestBuilder::consumes(...)` removed.** Use
  `.requires(handle, contract)` instead.
- **`log_legacy_consumes` removed from `runtime::lifecycle`.**
  Phase 5 of the boot orchestrator is now a no-op marker;
  the 8-phase numbering is preserved for diff stability.

### Phase 7: Tech-debt follow-up

- **Removed orphan `DependencyRef` public type.** The
  `pub struct DependencyRef { plugin, version, capability }`
  defined in `src/host/manifest/types.rs` and re-exported
  via `host::manifest` and `host` was zero-referenced after
  the Phase 6 `consumes` removal; the resolver walks
  `[[requires]]` exclusively and uses
  `CapabilityRequirement`, not `DependencyRef`. The struct
  and both re-exports are gone.
- **Removed `removed_quota_field_warnings` and D5
  warning-capture tests** (no longer needed: quota fields
  removed in Phase 5). The loader no longer scans the
  source for `tokens_per_minute` / `bytes_per_minute` —
  serde's default `deny_unknown_fields = false` already
  ignores the dead keys silently, so the diagnostic was
  redundant. The four `d5_warning_capture_*` tests that
  exercised the warning builder directly are deleted;
  the schema-level guards
  (`quota_spec_has_no_token_or_byte_fields`,
  `manifest_with_dead_quota_fields_still_loads`)
  remain.
- **`ruin_runtime_plugins` name is the deliberate dual of
  `mint_runtime_plugins`.** Despite an apparent asymmetry with
  the surrounding `teardown` / `[shutdown]` terminology in the
  same file, the function name is the intentional English-verb
  counterpart of `mint`. The seL4 mapping table treats
  `CNode.Mint` (creation) and `CNode.Delete` / `CNode.Revoke`
  (destruction) as the cap-level operations; the runtime layer
  abstracts the lifecycle one level up, and `ruin` (the
  dramatic opposite of "mint") is the runtime-layer term for
  the wholesale teardown of a plugin's minted capabilities in
  reverse mint order. The `[shutdown]` log prefix and the
  `teardown` module name are layer-local terms; the function
  name carries the duality. Phase 4 introduced it as
  `shutdown_runtime_plugins`; commit `ee680e3` renamed it to
  `ruin_runtime_plugins`. Future contributors: do not
  normalise this name to `teardown_runtime_plugins` or
  `destroy_runtime_plugins` without an explicit owner
  decision.

### Added — Phase 3: Capability-Native Runtime

Phase 3 proves the entire runtime can be built on Capability.
The completion criteria are seven experiments P3.1–P3.7 (P3.8 —
Sandbox-as-Capability-Env — is deferred to Phase 4 research
track). **All seven ship in this release**:

- **P3.1** Capability Injection — contract-keyed resolver +
  `[[requires]]` graph; replaces the legacy `[[consumes]]`
  plugin-version-keyed model.
- **P3.2** Protocol as Metadata — split `CapabilityContract`
  into load-bearing `AuthorityContract` (action → op map) and
  pure-metadata `Protocol` (schemas, transport).
- **P3.4** Capability-Native Agent — `AgentResource`'s
  reachable set derived from the binding table; no `SlotId`s
  in the agent's view.
- **P3.6** Runtime Lifetime — reverse-mint-order teardown via
  `cspace.revoke_tree`; consumers die before providers.
- **P3.7** Graph Events — `tokio::sync::broadcast` bus
  surfacing every mutation (`Minted`/`Derived`/`Revoked`/
  `RevokeTree`) and lifecycle event (`PluginActivated`/
  `PluginDeactivated`/`ShutdownStarted`/`ShutdownCompleted`).

#### P3.1 — Capability Injection

- **`CapabilityRequirement` manifest block** (`src/kernel/manifest.rs`):
  plugins declare what *contracts* they need, not which plugin
  versions provide them. `[[requires]] name = "..." contract = "..."`.
- **`contract_name` on `CapabilityDecl`** (`src/kernel/manifest.rs`,
  `src/capability/types.rs`): each `[[exposes]]` block now carries
  a `contract_name` string. Empty string means "no contract
  published" (legacy caps; only reachable by direct slot lookup).
- **`CapabilityMeta.contract_name`**: the contract name round-trips
  from the manifest through `meta_from_decl` into runtime metadata.
- **`resolver` module** (`src/kernel/resolver.rs`): capability-keyed
  dependency resolution. Builds a `contract_name → provider`
  index, walks every `[[requires]]`, runs Kahn's algorithm on the
  resulting edge graph to produce a deterministic topological
  mint order, and returns a per-plugin `Vec<ResolvedBinding>`.
  Detects `Unprovided`, `Ambiguous`, and `Cycle` errors.
- **Resolver wired into boot** (`src/boot/boot.rs`): the runtime
  mint sequence is no longer hand-written. After loading manifests
  and providing core services, `resolver::resolve(&manifests)`
  produces the mint order; `mint_runtime_plugins` iterates it
  and dispatches to the typed handler for each runtime plugin.
  Test-only plugins (counter, broker, channel, agent) are
  resolved and registered but not minted at boot — they remain
  test-driven.
- **PluginId gains `Ord`/`PartialOrd`** to support `BTreeMap`-keyed
  storage in the resolver. Ordering is `(name, version)` lexicographic.
- **`tests/epsilon/`** — new integration test crate. Eight tests
  covering: contract-keyed binding (ε.1), plugin version swappability
  (ε.2), `CapabilityMeta.contract_name` carry-through (ε.3),
  unprovided/ambiguous/cycle errors (ε.4–ε.6), diamond topology
  (ε.7), and a guard test that every runtime manifest declares a
  non-empty `contract_name` (ε.8).
- **All 11 runtime manifests updated** with `contract_name = "..."`
  on every `[[exposes]]` block. The `channel` plugin's two
  exposes publish distinct contracts (`channel` and `consumer`).

### Migration notes

- Manifests that omit `contract_name` still parse (default empty
  string) but their capabilities are unreachable via capability
  injection. Existing test manifests (`tests/common/mod.rs`) still
  work unchanged.
- The legacy `[[consumes]]` block (plugin-version-keyed) is still
  parsed and validated at boot. New plugins should prefer
  `[[requires]]`.
- Boot log now includes the resolved mint plan, so operators can
  see the contract bindings at startup.

#### Manifests as Rust constants

- Each runtime plugin now declares its manifest as a Rust
  constant returned by `pub fn manifest() -> &'static
  PluginManifest`. The boot sequence collects these via
  `boot::load_manifests` instead of parsing a `.toml` file.
  This:
    - gives the compiler the full set of fields to check
      (no more "did you forget `contract_name`?" surprises),
    - removes the toml parsing step from the runtime path,
    - lets `cargo doc` and IDE tooling follow plugin
      identity through the codebase.
- The `*.toml` files for runtime plugins are **deleted**.
  Test_only plugins still carry toml manifests because the
  test crates reach them via `PluginManifest::from_path`,
  exercising the parser end-to-end as a regression check.
- Construction pattern: each manifest is cached in a
  `OnceLock` because `PluginManifest` contains `String`
  fields and `String::from` is not a `const fn` on stable.
  Returning `&'static PluginManifest` from a memoised function
  is the simplest path that works on stable without the
  `inventory` crate (which requires `const` static
  initialisers). When Rust stabilises `const_heap` and
  `inventory` becomes viable, the `OnceLock` indirection
  can collapse.
- `tests/epsilon/p3_1_injection::runtime_manifests_have_contract_names`
  now collects manifests from **both** sources: runtime
  plugins via their `manifest()` fns (7 plugins), and
  test_only plugins via the toml walker (4 plugins). It
  verifies every one of the 11 declares a non-empty
  `contract_name` and resolves cleanly.

#### Manifest builder (`src/kernel/manifest_builder.rs`)

- A first attempt at a `plugin_manifest!{}` macro hit
  `macro_rules!` depth restrictions: four overlapping
  optional fields plus nested optionals inside `expose:
  { ... }` exceeded the metavariable-depth rules. The
  macro file (`manifest_macro.rs`) was abandoned.
- Replaced with `ManifestBuilder`, a fluent builder with
  per-field setters. Each runtime plugin's `manifest.rs`
  is now 4-8 lines of builder calls instead of 30+ lines
  of struct-literal boilerplate:
  ```rust
  ManifestBuilder::new("echo", "echo", "echo")
      .host("dispatcher")
      .timeout_ms(5000)
      .build()
  ```
  Defaults fill in `version = "0.1.0"`, `in/out_type = "any"`,
  `streaming = false`, empty `requires/consumes/host`,
  `timeout_ms = None` (host default 5000ms at mint time).
  Builder methods: `.version()`, `.in_type()`, `.out_type()`,
  `.streaming()`, `.requires()`, `.consumes()`,
  `.host()`, `.timeout_ms()`, `.action(name, op)`,
  `.build()`.

### P3.4 — Capability-Native Agent

The first consumer of the P3.1 binding table.

**Changes**

- `AgentResource` rewritten: storage is now `Vec<Reachable>`
  (each entry `{ handle, capability }`) instead of
  `Vec<(String, SlotId)>`. The reachable set comes from
  `ResolvedPlan::bindings[consumer]` via the new
  `AgentResource::from_bindings` constructor; the agent
  never sees slot identifiers.
- Dispatch path:
  `target → find Reachable by handle → cspace.lookup_by_name(reachable.capability)`.
  If `target` isn't in the reachable set, dispatch rejects
  with `"not in the binding table"`. If the binding exists
  but the cap is missing in cspace, dispatch rejects with
  `"reachable but cap missing in cspace"`.
- `boot.rs::mint_echo_chain` rewritten to read
  `plan.bindings[m.plugin]` instead of hardcoding
  `cspace.lookup_by_name("echo")`. The hardcoded lookup was
  the last "agent knows a cap name" code in the runtime
  path — it's gone now.
- `cspace.name_for_slot(SlotId) -> Option<String>` added:
  reverse lookup of the `names` map (registered name,
  not `meta.name`). For derived caps from `restrict`/
  `grant`, the registered name differs from the cap's own
  `meta.name` (which inherits from the parent).
- Legacy `agent::handler(name, slots, cspace)` constructor
  retained for β.4 back-compat. Internally translates each
  `(handle, SlotId)` to a `Reachable { handle, capability:
  cspace.name_for_slot(slot_id) }` so dispatch goes through
  the new binding-table path.
- `ManifestBuilder::action(name, operation)` added; folds
  each call into the capability's `CapabilityContract` at
  `build()` time.

**New tests** (`tests/zeta/p3_4_capability_native.rs`)

- **ζ.1.a** `same_binary_requires_counter_only_addresses_counter`:
  manifest `requires=[counter]` → reachable=`[counter]`;
  dispatch counter works, dispatch echo rejected.
- **ζ.1.b** `same_binary_requires_empty_addresses_nothing`:
  manifest `requires=[]` → reachable=`[]`; every target
  rejected even if a cap exists in cspace.
- **ζ.1.c** `same_binary_requires_two_handles_addresses_both`:
  manifest `requires=[counter, echo]` → reachable=`[counter,
  echo]`; both targets accepted.
- **ζ.2** `same_handle_different_capability_reaches_different_caps`:
  two agents with identical handle `"counter"` but
  bindings pointing at `"counter_read"` vs `"counter_write"`.
  Same handle, different reachable caps; agent A's
  `increment` fails (READ-only), agent B's succeeds
  (READ | WRITE).
- **ζ.3** `real_resolver_drives_reachable_set`: end-to-end
  with `kernel::resolver::resolve(&manifests)`, asserting
  `binding_for` exposes the real provider from the
  resolved plan.

**Test results**: 46/46 passing (was 41; +5 ζ tests).

### P3.6 — Runtime Lifetime

The runtime lifetime was previously asymmetric: plugins had
a structured **start** (mint order = topological) but a flat
**end** (`ctx.stop()` killed everything at once, no cap
revocation). P3.6 adds ordered teardown.

**Changes**

- `mint_runtime_plugins`, `mint_one_plugin`, `mint_simple`,
  `mint_echo_chain` now return `Vec<SlotId>` (the slots they
  minted for this plugin). `mint_runtime_plugins` collects
  them into `HashMap<PluginId, Vec<SlotId>>` and returns
  the whole map. The map is what makes the teardown direction
  explicit: every slot id that should be revoked on plugin X
  going down is tracked at mint time, not reconstructed later.
- New `shutdown_runtime_plugins(cspace, plan, minted)` walks
  `plan.mint_order` in **reverse** (consumers first) and calls
  `cspace.revoke_tree(slot_id)` for each minted slot.
- `boot.rs::run` calls `shutdown_runtime_plugins` after the
  HTTP bridge exits (replaces the bare `ctx.stop()`). End of
  the boot, cspace has zero runtime slots.

**Why reverse order**: consumers die **before** providers.
Any in-flight work the consumer was doing on the provider's
cap sees `Slot::capability() → None` rather than racing the
provider's teardown. For runtime plugins this is moot today
(they mint fresh caps and don't derive), but the rule
generalises cleanly when later phases add real provider
revocation hooks (think: live model swap, rolling restart).

**New tests** (`tests/zeta/p3_6_lifetime.rs`)

- **ζ.4** `teardown_order_is_reverse_of_mint`: 3 caps in a
  derived chain (root → 3 children). Teardown in reverse
  mint order; final `cspace.len() == 0`; every name cleared.
- **ζ.5** `provider_revoke_invalidates_consumer_reachable`:
  the **P3.4 ↔ P3.6 handshake**. Mint counter. Build an
  agent's binding table pointing at it. Dispatch works.
  Revoke the counter slot. Reachable entry in the agent
  is unchanged (it's metadata), but `lookup_by_name` returns
  `None`. Agent's invoke reports:
  `"reachable (handle=counter, capability=counter) but cap missing in cspace"`.
- **ζ.6** `revoke_tree_propagates_to_derived_caps`: 4-deep
  chain (root → read → read-only → write via restrict).
  `revoke_tree(root)` returns `4`, every level clears.
- **ζ.7** `consumer_teardown_does_not_revoke_provider`:
  revoking a phantom consumer slot (id `9999`, not in cspace)
  is a no-op and leaves the provider alive. Then revoking
  the provider makes the agent's reachable entry fail.

**End-to-end boot**

```
[shutdown] tearing down runtime plugins (reverse mint order):
  ✓ echo-chain@0.1.0  revoked 1 slot(s)
  ✓ slow@0.1.0  revoked 1 slot(s)
  ✓ sandbox@0.1.0  revoked 1 slot(s)
  ✓ reverse@0.1.0  revoked 1 slot(s)
  ✓ generator@0.1.0  revoked 1 slot(s)
  ✓ echo_stream@0.1.0  revoked 1 slot(s)
  ✓ echo@0.1.0  revoked 1 slot(s)
[shutdown] cspace remaining slots: 0
```

**Test results**: 50/50 passing (was 46; +4 ζ tests for P3.6).

### P3.2 — Protocol as Metadata

The old `CapabilityContract` struct conflated two concerns:
the **action vocabulary** (which the runtime path consulted
via `operation_for(action)` to translate verbs to
`OperationRights` bits), and the **wire-format metadata**
(schemas, description, transport hint). P3.2 splits them.

**Why split**

The two audiences are different. Type-agnostic dispatchers
(`RuleAgent`) need the action vocabulary — that's load-bearing
authority input. External clients (HTTP bridge, OpenAPI
generators, future JSON-RPC descriptors) need the wire
metadata — that's pure documentation. Forcing them into one
struct meant the wire metadata was implicit at every
authority-check site, and adding new metadata fields (transport,
version, media_type) risked polluting the authority path.

**Changes**

- New `AuthorityContract` type (in `capability::types`):
  - `actions: Vec<CapabilityAction>` (name → operation string)
  - `operation_for(action) -> Option<&str>` — the agent's
    only authority source
- New `Protocol` type:
  - `description: String`
  - `input_schema: Value`, `output_schema: Value`
  - `media_type: String`, `version: String`, `transport: String`
- `CapabilityMeta` carries both: `pub authority:
  AuthorityContract`, `pub protocol: Protocol`. The old single
  `contract` field is gone.
- `CapabilityDecl` (in `PluginManifest`) carries both; old
  `contract` field gone.
- TOML shape updated:
  - `[exposes.contract]` → `[exposes.protocol]`
  - `[[exposes.contract.actions]]` → `[[exposes.authority.actions]]`
- `ManifestBuilder::protocol(Protocol)` setter added.
  `.action(name, op)` continues to work, building authority.
- `RuleAgent` (the only authority-checker that uses these
  fields) reads `cap.meta().authority.operation_for(action)`.
  `meta.contract` references gone.
- **`Resource::invoke` trait unchanged.** Still takes raw
  `Value`; protocol is observable but never validated.

**Tests**

Updated γ.2–γ.6 to read from `protocol` (metadata) and
`authority` (action vocabulary). The contract_* files are
renamed in spirit (kept the same filenames for git history
clarity) but the assertions all read `cap.protocol.*` or
`cap.authority.*`.

New ζ tests (`tests/zeta/p3_2_protocol.rs`):

- **ζ.8** `dispatch_unchanged_with_or_without_protocol_metadata`:
  two caps with identical authority but different protocol
  metadata (different `media_type`, `version`, `transport`)
  produce identical dispatch results for the same input.
- **ζ.9** `protocol_queryable_from_cap_meta`: `slot.meta().protocol`
  carries description, schemas, media_type, version, transport
  faithfully through mint.
- **ζ.10** `authority_operation_for_is_action_vocabulary` and
  `authority_vocabulary_drives_dispatch_authority`: agent reads
  `meta.authority.operation_for(action)`, gets the bit string,
  parses it, checks against held ops. End-to-end through
  agent + cap + resolver.
- **ζ.11** `manifest_builder_protocol_roundtrips`:
  `ManifestBuilder::protocol(...)` produces the same fields
  as a hand-built struct; the `..Default::default()`-style
  pattern works.

**Test results**: 55/55 passing (was 50; +5 ζ tests for P3.2).

### P3.7 — Graph Events

The runtime had no observability hook — mint/revoke/teardown
happened silently. P3.7 adds a `GraphEventBus` backed by
`tokio::sync::broadcast` that cspace mutations and boot
lifecycle events publish to. Subscribers see the full
timeline in publish order, non-blocking.

**Why broadcast (not a callback Vec or sink trait)**

- **Non-blocking publish.** `Sender::send` returns
  immediately. The cspace stays sync; mint never waits on
  subscribers.
- **Multi-subscriber.** HTTP bridge SSE, log subscriber,
  test recorder, future audit log — each gets its own
  receiver without coordination.
- **Tokio-native.** The codebase already pulls tokio for
  the HTTP bridge and signal waits; reusing the runtime's
  primitives keeps the dependency surface flat.

**Changes**

- `src/capability/events.rs` (new): `GraphEvent` enum,
  `DeriveKind` enum (`Grant`/`Restrict`/`Transfer`),
  `GraphEventBus`, `GraphEventReceiver` re-export. Default
  capacity 256; tests can use smaller capacities.
- `CapabilitySpace` carries a `GraphEventBus` (defaults
  to `GraphEventBus::new()`). New convenience methods:
  `cspace.subscribe()`, `cspace.events()`.
- All cspace mutations publish events:
  - `install` → `Minted { plugin, slot, capability, contract }`
  - `grant`/`restrict`/`transfer` → `Derived { parent, child, kind }`
  - `revoke` → `Revoked { slot, capability }`
  - `revoke_tree` → one `Revoked` per slot + one
    `RevokeTree { root, total }` marker
- `boot.rs` publishes lifecycle events on the same bus:
  - `PluginActivated { plugin }` after cordis handler Ok
  - `PluginDeactivated { plugin }` before revoke
  - `ShutdownStarted` at the top of teardown
  - `ShutdownCompleted { remaining_slots }` after teardown

**New tests**

In `src/capability/events.rs` (unit, 4 tests):

- `publish_with_no_subscribers_is_dropped` — no panic,
  return the dropped event via `Err`
- `subscribe_then_publish_delivers` — basic fan-out
- `multiple_subscribers_each_get_event` — fan-out to N
- `event_ordering_with_single_sender` — pin FIFO ordering

In `tests/zeta/p3_7_events.rs` (5 tests):

- **ζ.12** `mint_fires_minted_event`: single mint produces
  one `Minted` with right fields
- **ζ.13** `revoke_tree_fires_revoked_per_slot_plus_one_revoke_tree`:
  N revokes + 1 RevokeTree per `revoke_tree(root)` call
- **ζ.14** `derive_paths_fire_distinct_kinds`: `grant` /
  `restrict` / `transfer` produce distinct `Derived` events
  in publish order
- **ζ.15** `every_subscriber_gets_every_event`: two receivers
  each see the same Minted + Revoked + RevokeTree sequence
- **ζ.16** `full_lifecycle_event_sequence`: simulated full
  boot (3 plugins mint → activate → shutdown reverse →
  completed) produces the expected 17-event timeline.

**Test results**: 64/64 passing (was 55; +4 events unit +
+5 ζ tests for P3.7).

### P3.3 — Real AI Resources

The `generator` plugin used to be a hard-coded mock:
detect "hello" in the prompt and emit canned text, otherwise
echo the prompt. P3.3 replaces that with a `Model` trait and
two real implementations.

**Why Markov, not a real LLM**

- Network I/O, API-key dep, and non-determinism all conflict
  with the streaming-quota / quota-spec test surface.
- Markov chain hits the same shape (variable-length stream
  of tokens that depends on the prompt) without those costs.
- The [`Model`] trait is what `generator`'s handler
  dispatches through; swapping in an HTTP-bridged LLM later
  is a one-file change.

**Changes**

- `src/plugins/generator/model.rs` (new): `Model` trait;
  `MockModel` (canned back-compat); `MarkovModel` (n-gram
  generator trained on a built-in tech/programming corpus).
- `MarkovModel` uses `StdRng` seeded by `prompt.hash()` XOR
  model seed → deterministic given `(model, prompt)`.
- `ModelKind` enum (`Mock` / `Markov`) parsed from
  `GENERATOR_MODEL` env var (default `Markov`).
- `GeneratorResource` carries `Arc<dyn Model>`; `handler(model)`
  takes the model as an argument.
- `boot.rs` reads `GENERATOR_MODEL` and prints
  `[generator] selected model: Markov` so operators can
  confirm the choice.
- `Cargo.toml`: `rand = "0.8"` (light dep, just `StdRng`).

**Tests**

In `src/plugins/generator/model.rs` (8 unit tests):

- `mock_model_canned_hello_response`
- `mock_model_echoes_non_hello`
- `markov_model_deterministic_for_same_prompt_and_seed`
- `markov_model_different_for_different_seeds`
- `markov_model_returns_non_empty_output`
- `markov_model_ngram_table_populated`
- `model_kind_parses_known_strings`
- `model_kind_build_returns_a_model`

In `tests/zeta/p3_3_real_models.rs` (9 tests):

- **ζ.17** `mock_model_preserves_historical_behaviour`
- **ζ.18** `markov_model_extends_prompt_and_is_deterministic` +
  `markov_model_varies_with_prompt` +
  `markov_model_force_seed_forces_output`
- **ζ.19** `model_kind_default_is_markov` +
  `model_kind_mock_when_env_says_mock` +
  `model_kind_markov_when_env_says_markov`
- **ζ.20** `runtime_handler_streams_mock_model_in_order` +
  `runtime_handler_streams_markov_model_with_real_output`

**End-to-end HTTP bridge**

```
# Markov:
$ curl -X POST http://127.0.0.1:3030/api/stream \
    -d '{"capability":"generate","input":"the kernel"}'
event: chunk
data: "the"
event: chunk
data: "kernel"
event: chunk
data: "enforces"
event: chunk
data: "the"
event: chunk
data: "bits"
event: chunk
data: "[end]"
event: done

# Mock (back-compat, $GENERATOR_MODEL=mock):
event: chunk
data: "Mock"
event: chunk
data: "generator"
event: chunk
data: "received:"
... (echo with prefix)
```

**Test results**: 82/82 passing (was 64; +8 model unit +
+10 ζ tests for P3.3).

### Added — Phase 2: Capability-Native Composition

#### Plugin directory cleanup

- **`src/plugins/test_only/`** — counter, broker, channel, agent
  moved here. They are no longer loaded by `boot::run` (the
  manifest walker skips the `test_only/` subtree) and no longer
  appear in the `[manifest]` boot log. They remain reachable
  from tests as `odyssey::plugins::test_only::counter::*` etc.
- **`src/plugins/echo/` family grouping** — the three
  echo-family plugins are now nested under one parent:
  ```
  src/plugins/echo/
  ├── mod.rs        — parent, declares submodules
  ├── basic/        — was src/plugins/echo/         (sync passthrough)
  ├── chain/        — was src/plugins/echo_chain/   (sync chained)
  └── echo_stream/  — was src/plugins/stream_echo/  (renamed, streaming passthrough)
  ```
  The streaming echo plugin was renamed `stream_echo` →
  `echo_stream` so the whole family shares the `echo_` prefix
  (echo / echo_chain / echo_stream) and is greppable as one
  shape. Plugin identifier, capability name, contract name,
  struct name (`StreamEchoResource` → `EchoStreamResource`),
  plugin factory fn (`stream_echo_plugin` →
  `echo_stream_plugin`), and slot key (`slot:stream_echo` →
  `slot:echo_stream`) all moved together.
- **`src/plugins/mod.rs`** — runtime plugins (echo family,
  generator, reverse, sandbox, slow) are declared at the top
  level; test-only plugins live under the new
  `pub mod test_only { ... }` submodule.
- **`src/plugins/mod.rs`** — runtime plugins (echo family,
  generator, reverse, sandbox, slow) are declared at the top
  level; test-only plugins live under the new
  `pub mod test_only { ... }` submodule.
- **`src/boot/boot.rs`** dispatch slimmed: a single
  `RUNTIME_PLUGINS` const drives both the mint gate and the
  activator dispatch (`activator_for`). The previous triple
  (`RUNTIME_PLUGINS` + `mint_one_plugin` match + `plugin_factories`
  HashMap) is now a const + two match arms, with a debug_assert
  that the activator covers every name in the const. Adding a
  new runtime plugin still requires touching two places
  (const + mint arm + activator arm) but the compiler now
  catches "added to const but forgot activator".
- **`echo_chain.toml`** now declares its real dependency via
  `[[requires]]`:
  ```toml
  [[requires]]
  name     = "echo"
  contract = "echo"
  ```
  Boot log proves the resolver binds it: `echo-chain@0.1.0
  receives: handle=echo contract=echo from=echo@0.1.0
  (cap=echo)`. The legacy `[[consumes]]` block is kept for
  back-compat.

### Added — Phase 2: Capability-Native Composition

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

### Added — Phase 5: kernel / host / runtime split

Phase 5 reorganises the source tree into three orthogonal
layers — `kernel` (pure capability algebra), `host` (manifest
composition + dependency resolution), and `runtime` (lifecycle
adapter + HTTP bridge). The split is the prerequisite for
shipping the kernel as a library that does not depend on
`tokio::time::Instant::now()` direct calls, on the filesystem,
or on the cordis plugin bus.

```
src/
├── kernel/   pure capability kernel (no I/O, no Instant::now)
├── host/     manifest loader + resolver + factory + pipeline
├── runtime/  lifecycle + activate + teardown + http_bridge
└── plugins/  plugin bodies (depend on kernel + host)
```

#### Defect fixes landed in this phase

- **D1** — `kernel/cap/typed.rs::derive` doc compressed to a
  tripwire `debug_assert!`; precondition is now `cspace::install_derived`
  has already verified `held.contains(&rights)`.
- **D2** — `kernel/space/mod.rs::install_derived` refuses when the
  parent id is no longer in `slots` (race-condition guard against
  Interleaving 2). The check is against `slots`, not `parents`,
  so root caps are not wrongly rejected.
- **D3** — `kernel/quota/state.rs::CapabilityBudget::share_with`
  clones the parent's `Arc<AtomicU64>` and `Arc<QuotaState>`
  rather than allocating fresh. Derived caps now report
  subtree-wide wall-clock usage; the per-minute call window is
  global across the subtree.
- **M1** — `kernel/space/mod.rs::install` takes `parents.write()` as
  a no-op lock first, matching the canonical `parents → slots →
  names` order so concurrent `revoke_tree` waits on the write
  lock until the insert completes.
- **M2** — `kernel/cap/erased.rs::AnyCapability::set_revoked_dyn`
  default impl panics. Every `AnyCapability` impl MUST override
  this to flip a marker that the dispatch path checks — silent
  fall-through was the original soundness gap.
- **M3** — `kernel/cap/typed.rs::invoke` + `invoke_op` reorder:
  handler runs first; on success the elapsed wall-clock is recorded
  (`self.clock.now()`); then the per-call timeout is checked (return
  `CapabilityError::Timeout` if the budget was blown, dropping the
  successful result — the budget is the contract); then the
  per-minute quota is debited (`CapabilityError::QuotaExceeded` if
  the bucket is full); then the handler result is returned. The
  budget contract is the contract: a late answer is not a correct
  answer. `CapabilityError::Timeout` variant added for the
  late-timeout case. `Capability<R>` carries its own `Arc<dyn Clock>`
  so the timeout recording does not call `Instant::now()` directly
  (P1-C).
- **M4** — typed `CapabilityError` variants replace substring
  matching on the rendered error message. `host/pipeline.rs`
  pattern-matches on `Revoked(SlotId)` / `SlotEmpty(SlotId)` via
  the new `AnyCapability::invoke_dyn_typed` method (returns
  `Result<Value, CapabilityError>`).
- **m3** — `kernel/slot.rs::Slot::kind()` removed; callers use
  `cap.kind()` on the capability itself.
- **m6** — `runtime/http_bridge.rs::router` no longer takes a
  `cordis::Context` parameter that was always suppressed.
- **n1** — `kernel/slot.rs` three `"slot X empty or revoked"`
  format! strings replaced with `CapabilityError::SlotEmpty(id).to_string()`.
- **n3** — `kernel/quota/state.rs::quota_state` field is now
  private; the public surface is `snapshot()` + `pub(crate) try_call()`.
- **R1** — `kernel/space/namespace.rs` is the single home for
  `namespace_prefix_matches`. The Phase 4
  `capability::graph::namespace_starts_with` is gone.
- **R4** — `kernel/space/derivation.rs::grant`/`transfer`/`restrict`
  share the same `derive_with` body. Phase 4's
  `revoke_with_sweep` is gone.
- **R5** — `host/resolver/{error,index,topo,plan}.rs` split; each
  phase independently testable.
- **R6** — `kernel/space/revocation.rs::revoke` takes a
  `RevokeMode::{Single, Tree}` selector.
- **R7** — `kernel/space/derivation.rs` shared `derive_with`
  body for grant / transfer / restrict.

#### Phase 5 breaking changes

- **Manifest fields removed (D5):** `tokens_per_minute` /
  `bytes_per_minute` (and their `with_*` builders) are gone
  from `QuotaSpec`. The kernel no longer tracks these quotas.
  `QuotaKind::Tokens` / `QuotaKind::Bytes` variants are gone;
  `QuotaState::try_tokens` / `try_bytes` are gone. Manifests
  that previously set those fields now have the limits
  dropped on load with a stderr warning (the loader emits a
  per-field diagnostic so operators on upgraded manifests
  see the change).
- Import paths: `odyssey::capability::*` is gone. The kernel
  types live at `odyssey::kernel::*`, the cspace at
  `odyssey::kernel::space::*`, the host composition layer at
  `odyssey::host::*`. There is no `odyssey::capability::*`
  namespace alias — every test file and plugin was migrated to
  the canonical paths.
- `Capability<R, K>` is now `Capability<R>` with `CapKind`
  tracking Sync vs Stream at runtime instead of at the type
  level. Slot references drop the `K` parameter (`Slot<R>`).
- `invoke_op` returns `Result<Value, CapabilityError>` instead
  of `Result<Value, String>`. The typed variants are the
  canonical error shape; stringly-typed errors only surface
  from `invoke_dyn` (and the host pipeline's `StageFailed`
  carries the inner `String` so handlers can still raise
  domain-specific failures).
- `set_revoked(bool)` replaces `mark_revoked` / `reset_revoked`
  on `Capability<R>`; `set_revoked_dyn(bool)` replaces
  `mark_revoked_dyn` on `AnyCapability`. The default `set_revoked_dyn`
  impl panics (M2 invariant preserved).
- `ModelKind::with_http(cspace, reachable)` replaces
  `ModelKind::build_http(...)`. The builder pattern uses `build`
  for the no-cap variant and `with_*` for variants that need a
  cap.
- `host/pipeline.rs::PipelineError::SlotRevoked` is now
  decided by pattern-matching on the typed `CapabilityError`
  variant, not by substring matching on the rendered message.
- `runtime/` no longer carries the Phase 4 `boot/` submodule;
  the boot path moved into `runtime::lifecycle` + `runtime::activate`
  + `runtime::mint`.
- `plugins/agent/handler.rs` (712 LOC) split into five focused
  files: `handler.rs` (struct + constructors + parse_operation),
  `dispatch.rs` (sync `Resource::invoke` body), `stream.rs`
  (async `Resource::open` body + `run_program` + `RunStats` +
  event-emission helpers), `plugin.rs` (cordis glue), `panic.rs`
  (`panic_payload_to_str`). Public surface (`agent_handler`,
  `handler_from_plan`, `agent_plugin`, `AgentResource`,
  `ProgramStep`) unchanged.
- `kernel/space/graph.rs::NamespaceNode::capabilities` field
  removed. The flat `CapabilityGraph::nodes` array is the
  single source of truth for capability nodes; consumers
  join by `namespace` when they need a per-namespace
  grouping.

## [0.1.0] — initial

- Initial seL4-style capability kernel with `CapabilityToken` and
  string-based dispatch.
