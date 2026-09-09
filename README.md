# odyssey

A seL4-style capability kernel implemented in Rust, with manifest-driven
plugin loading. Plugins declare the capabilities they expose and consume;
the host mints unforgeable `CapabilityToken`s, provides them into the cordis
DI context, and validates the dependency graph before any plugin activates.

## Layers

| seL4                      | odyssey                                              |
| ------------------------- | ---------------------------------------------------- |
| `CNode.Allocate`          | `CapabilityFactory::mint_sync` / `mint_stream`       |
| CNode capability (handle) | `CapabilityToken` (`Arc<CapabilityToken>`)           |
| `endpoint.send`           | `token.invoke()` / `token.stream()`                  |
| resource badge            | `CapabilityBudget.timeout_ms` (per-call wall clock)  |

## Boot order

1. Parse manifests
2. Provide core services (`factory`, `capability_service`, `registry`)
3. Mint all tokens, in dependency order
4. Provide tokens (`ctx.provide("cap:{name}", token)`)
5. **Fail-fast** dependency check — abort boot if any `consumes` is missing
6. Start plugin fibers — cordis resolves `inject` declarations
7. Demo harness — invoke via tokens
8. HTTP bridge on `127.0.0.1:3030`

## HTTP bridge

```
GET  /api/caps    →  enumerate registered capabilities
POST /api/invoke  →  invoke a sync capability
POST /api/stream  →  open a streaming capability (SSE)
```

## Plugins

Each plugin lives in `src/plugins/<name>.rs` and exports:
- `pub fn handler() -> Arc<dyn SyncInvoke>` (or `Arc<dyn StreamInvoke>`)
- `pub fn <name>_plugin() -> Arc<dyn Plugin>`

The plugin manifest lives in `plugins/<name>.toml` and declares identity,
isolation, exposed capabilities, dependencies, host services, and resource
hints. The host reads the manifest, dispatches to the right plugin module
by name, mints a token wrapping the plugin's handler, and provides it into
the cordis context.

## Build

```sh
cargo build
cargo run
```