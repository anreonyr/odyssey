# Scope: WASM / cdylib dynamic plugin loader

> Deferred slot from Slice 5 (`874f543`) — `load_plugin_from_path`
> returns a `LoadedPlugin` whose `MintFn` is a placeholder that panics.
> This document scopes the follow-up that wires a real handler binder.

## Goal

Load third-party plugin manifests from disk and execute their
declared capabilities through the kernel's existing
capability-mediated invoke path. The `MintFn` produced by
`load_plugin_from_path` must dispatch into a real `Resource` impl
backed by either a native cdylib or a WASM module, so a manifest
written by an external author — outside the `odyssey` source tree —
can register capabilities the same way a builtin does.

Concretely: replace the `placeholder_mint_fn` panic in
`crate/odyssey/src/personality/lifecycle/loader.rs` with a binder
that, given `manifest.exposes[i].name`, calls the matching handler
inside the loaded module and returns a `SlotId` whose backing
capability delegates to that handler at invoke time.

## State of the deferred material

`docs/deferred/` already carries the design residue from earlier
attempts. We are not redoing that work; we are scoping the path
that turns the sketches into a loader:

- `wasm-loader.wat` — **the useful half.** A real, valid module
  that pins the host ABI: exported `memory`, `name(out_buf, cap,
  out_len)`, `invoke(in_buf, in_len, out_buf, out_cap, out_len)
  -> i32` with return codes `0 = ok`, `1 = small buffer`, `2 =
  invalid input`. That contract is what both a WASM module and a
  cdylib need to satisfy.
- `wasm-loader.toml` and `cdylib-loader.toml` — sketches that
  predated Phase 8 / Phase 9. Every section (`[isolate]`,
  `[source]`, `streaming = false`) is stale; the readmes on each
  file say so explicitly. They are not inputs to anything.
- The readmes' caveat that the "echo plugin" referenced from the
  TOMLs was deleted in Phase 8 (now `builtins/src/echo.rs`)
  still holds.

The Phase 9 audit (`CHANGELOG.md` Phase 7 / 8 entries) already
recorded `wasmtime` + `wat` as removed optional deps and
`libloading` as unwired. Re-adding either is a dependency
decision this scope document defers to the implementation
commit.

## Two approaches

### A. cdylib via `libloading`

`dlopen` / `LoadLibrary` on the plugin's compiled `.so` / `.dll`,
look up the `name` and `invoke` symbols by name, call them across
the FFI boundary.

- **Pros**
  - Tiny dep. `libloading` is a thin wrapper over the platform
    dynamic loader; no compiler, no runtime, no extra build
    step on the host side.
  - Native ABI performance — no marshalling layer between the
    host and the plugin beyond the buffer protocol.
  - Plugin author writes ordinary Rust (or any language that can
    emit a C-ABI cdylib). Existing Rust toolchain experience
    applies directly.
  - The C-ABI buffer protocol is the *same* one `wasm-loader.wat`
    documents, so the host-side plumbing is shared with the WASM
    path later.
- **Cons**
  - **No sandbox.** The cdylib runs in the host process with
    full syscall access. The kernel's capability-mediated invoke
    path constrains *what the loaded module can do via the
    capability it receives*, but the cdylib can still open files,
    make network sockets, fork, etc. Capability mediation is
    not a sandbox; it is a cap filter on the *call surface*.
  - ABI brittleness: `extern "C"` is stable, but any Rust type
    crossing the boundary (`&[u8]`, `String`, etc.) is not. The
    plugin ABI must be plain C types only — `*const u8`,
    `*mut u8`, `usize`, `i32` — and the plugin must be built
    with the same Rust toolchain version as the host (or, more
    carefully, with a toolchain pinned to a host-ABI target).
  - A panicking / aborting cdylib takes the kernel down. The
    only safe wrapper is `std::panic::catch_unwind` *plus* a
    rule that plugin code uses `panic = "unwind"` (not
    `"abort"`); FFI calls into a cdylib built with
    `panic = "abort"` are unrecoverable.
  - Crashes inside the cdylib propagate into the host's unwind
    tables and can leak state.

### B. WASM via `wasmtime`

Compile the plugin to a WASM 2.0 component, instantiate it through
`wasmtime::Engine` + `Linker`, call its `name` / `invoke` exports
via the wasmtime API. Fuel and epoch interruption enforce CPU
bounds; the `Linker` denies any host import not explicitly
registered; memory is bounded by declared limits.

- **Pros**
  - **Sandbox by default.** The plugin cannot make syscalls; it
    can only call host imports we choose to expose. A plugin
    that calls `wasi_snapshot_preview1::fd_write` simply fails
    to link if `wasi` is not in the linker.
  - Deterministic resource bounds: fuel metering makes a
    malicious infinite loop terminate with an error rather than
    hang the kernel. Memory is bounded by `max_memory_size` on
    the instance config.
  - A WASM trap returns an error to the host; the host process
    stays up.
  - The host ABI is the same one `wasm-loader.wat` already
    implements (exported `memory`, `name`, `invoke` with
    return codes 0/1/2). `wasm-loader.wat` is, in effect, the
    reference implementation of the contract.
- **Cons**
  - Heavy dependency: `wasmtime` pulls Cranelift (JIT or
    interpreter), `wasmparser`, `wast`, `wit-parser`, the
    component-model machinery, and several transitive crates.
    Compiled binary size grows by several MB; build times grow
    noticeably. Whether JIT is enabled (`default-features`) or
    only the interpreter changes this materially.
  - **WIT / import interface design is its own piece of work.**
    Every host function the plugin can call must be declared
    in a WIT file and implemented in the linker. For an MVP
    that means at minimum: a way to read the plugin's own
    capability set (or a slice of it), a clock source, and a
    log sink. Anything else — file I/O, network, inter-plugin
    messaging — is a separate design decision with security
    implications.
  - Marshalling cost. Every `invoke` crosses a boundary; large
    payloads need shared linear memory and a defined ownership
    protocol. Less of an issue than it sounds (the C ABI needs
    the same buffer dance) but non-zero.
  - Cold-start compile of the module per plugin. Cacheable but
    still a one-shot cost per load.

## Recommended direction

**Land cdylib first, then add WASM as a second backend to the
same loader.**

Rationale:

1. The placeholder gap from Slice 5 is small and self-contained.
   A cdylib backend is the fastest path to a non-panicking
   `MintFn` for loaded plugins, and the kernel's existing
   `CapabilityFactory::mint<R>` plumbing (see
   `personality/lifecycle/mint.rs`) does not change.
2. The host ABI the cdylib must implement is identical to the
   one `wasm-loader.wat` already documents. Building cdylib
   first produces a host-side buffer / framing implementation
   that the WASM backend will reuse verbatim. We are not
   throwing away work.
3. The kernel's capability model already mediates what a loaded
   module can *do through its granted caps*. For trusted
   in-tree plugins, that mediation plus a vetted cdylib is
   defensible. WASM is needed when we accept code from outside
   the source tree, which is a *later* product question, not a
   Slice-5-follow-up blocker.
4. cdylib avoids the WIT design work entirely for the first cut.
   WIT is a real cost (a few days of design + a WIT file + a
   linker impl for each host function), and it is the kind of
   work that benefits from knowing the concrete host functions
   the first cdylib plugin actually calls. Land cdylib first,
   then the WIT design is grounded in observed use.

The end state has both: `kind = "cdylib"` for trusted in-tree
plugins (fast, no JIT cost), `kind = "wasm"` for third-party
plugins (sandboxed). The loader dispatches on `kind`.

This is the honest version. If the priority were *untrusted
plugins* above all else, WASM would be the recommendation; the
trade-off is that the deferred slot was opened by Slice 5's
in-process placeholder, and the smallest fix that removes the
placeholder is cdylib.

## Manifest schema additions

Add a top-level `backend` block to `PluginManifest`. `builtin`
is implicit (a manifest with no `backend` block is a builtin —
the current behaviour). The block is what tells the loader to
look for an external module.

```json
{
  "plugin": { "name": "echo-cdylib", "version": "0.1.0" },
  "backend": {
    "kind": "cdylib",
    "path": "target/debug/libecho_cdylib.so",
    "abi_version": 1
  },
  "exposes": [
    {
      "name": "echo",
      "kind": "sync",
      "contract_name": "echo",
      "tool_schema": null
    }
  ],
  "requires": [],
  "timeout_ms": null
}
```

```json
{
  "plugin": { "name": "echo-wasm", "version": "0.1.0" },
  "backend": {
    "kind": "wasm",
    "path": "plugins/echo.wasm",
    "abi_version": 1
  },
  "exposes": [
    { "name": "echo", "kind": "sync", "contract_name": "echo" }
  ]
}
```

```rust
// New types, additive (serde `default` keeps old manifests valid).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginManifest {
    pub plugin: PluginId,
    #[serde(default)]
    pub backend: Option<BackendDecl>,   // <-- new; None == builtin
    #[serde(default)]
    pub exposes: Vec<CapabilityDecl>,
    #[serde(default)]
    pub requires: Vec<CapabilityRequirement>,
    #[serde(default)]
    pub timeout_ms: Option<u32>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BackendKind { Builtin, Cdylib, Wasm }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BackendDecl {
    pub kind: BackendKind,
    pub path: std::path::PathBuf,
    #[serde(default = "default_abi_version")]
    pub abi_version: u32,
}

fn default_abi_version() -> u32 { 1 }
```

`validate()` is extended to reject `path` that is not absolute
or that escapes the plugin directory, and to require
`abi_version == 1` (only the current version is recognised). The
existing validator in
`crate/odyssey/src/core/manifest/manifest.rs` is the place to
add this.

We deliberately do *not* add a `kind` field to `CapabilityDecl`.
A plugin is one module; its `exposes` are entries in that one
module. Per-capability backends would invite per-capability
module loading, which is not what we need.

## Loader wiring

`load_plugin_from_path` becomes:

```rust
pub fn load_plugin_from_path(path: &Path)
    -> Result<LoadedPlugin, ManifestLoadError>
{
    let manifest = PluginManifest::from_path(path)?;
    let backend = manifest.backend.clone()
        .unwrap_or(BackendDecl { kind: BackendKind::Builtin,
                                path: PathBuf::new(),
                                abi_version: 1 });
    let LoadedPlugin { manifest, mint_fn, ruin_fn, handle } =
        match backend.kind {
            BackendKind::Builtin => {
                // No external module. Caller is expected to
                // supply the MintFn themselves; here we still
                // return placeholder_mint_fn so the error path
                // is loud.
                LoadedPlugin::builtin(manifest)
            }
            BackendKind::Cdylib => cdylib::load(manifest, &backend)?,
            BackendKind::Wasm   => wasm::load(manifest, &backend)?,
        };
    Ok(LoadedPlugin { manifest, mint_fn, ruin_fn, handle })
}
```

`LoadedPlugin` grows an opaque handle for teardown:

```rust
pub struct LoadedPlugin {
    pub manifest: PluginManifest,
    pub mint_fn: MintFn,
    pub ruin_fn: RuinFn,
    handle: Option<LoadedHandle>,   // private; consumed by ruin
}

enum LoadedHandle {
    Cdylib(libloading::Library),
    Wasm {
        _engine: wasmtime::Engine,
        _store:  wasmtime::Store<()>,
        // linker / instance kept alive for the plugin's lifetime
    },
}
```

The cdylib `MintFn` body:

1. The plugin's exported `invoke` symbol is looked up once at
   load time and stashed in a `OnceLock` per capability name.
2. At invoke, the `Resource` impl serialises the input to a
   buffer (`postcard` or JSON — a separate small decision),
   calls the symbol with the `(in_buf, in_len, out_buf,
   out_cap, out_len) -> i32` ABI, reads the result buffer,
   deserialises, and returns the typed value.
3. The whole call is wrapped in `std::panic::catch_unwind`. If
   the cdylib panics through the FFI boundary with unwinding
   semantics, we surface a `HandlerPanicked` error instead of
   unwinding the kernel.

The WASM `MintFn` body is the same shape — serialise, call the
exported `invoke`, deserialise — but goes through
`wasmtime::TypedFunc` and reads / writes linear memory through
the host accessor. A trap becomes a `wasmtime::Trap` error
propagated to the caller; no `catch_unwind` needed.

`RuinFn` drops the `LoadedHandle`, which drops the
`Library` / wasmtime `Store` and tears down the module. The
default `RuinFn` for loaded plugins is therefore *not*
`default_ruin`; it is the per-backend unload hook. Builtins
keep `default_ruin`.

## Security model

The kernel already mediates everything that crosses a capability
boundary at invoke time. That mediation is the security model
for *capability use*, not for *syscall access*, and that
distinction is what this scope has to be honest about:

- **Builtin / in-tree cdylib:** trusted. Runs in the host
  process; the cap filter constrains what the loaded module can
  *do through the cap it was given*; it does not constrain
  arbitrary syscall use outside the cap. Acceptable because the
  source is in our tree and code review applies.
- **WASM:** the loader creates a `wasmtime::Engine` with fuel
  metering enabled and an epoch interruption deadline. The
  `Linker` exposes only the host functions the WIT file lists;
  `wasi_snapshot_preview1` is **not** linked by default. Memory
  is bounded. A WASM trap returns an error; the host process is
  not affected.
- **Per-call isolation:** the cdylib `invoke` call is wrapped
  in `std::panic::catch_unwind` so a panicking plugin cannot
  unwind the kernel. The wrapper is only correct when the
  cdylib is built with `panic = "unwind"`; this is documented
  in the plugin author guide.
- **Path validation:** the loader rejects relative paths,
  paths containing `..`, and symlinks that resolve outside the
  configured plugin root. (The validation logic is the same as
  what `validate()` would do for any other on-disk resource;
  it's the same attack surface.)

No new privilege escalation: the cap is what the *plugin*
owns, the loaded module receives it, and the loaded module's
return value is what the kernel sees — same as a builtin
today.

## Estimated scope

For **cdylib only** (recommended primary path):

- **Week 1**: Manifest schema (`BackendDecl`, `BackendKind`),
  `validate()` extension, serde round-trip tests. Loader
  dispatch skeleton (`match backend.kind { Builtin | Cdylib |
  Wasm }`); only `Builtin` and `Cdylib` arms have real code.
- **Week 2**: cdylib loader. `libloading::Library::new`, symbol
  lookup for `name` and `invoke`, ABI negotiation via
  `abi_version`. Buffer framing module (serialise /
  deserialise payloads — pick `postcard` vs JSON here, this is
  a one-day decision).
- **Week 3**: `MintFn` body for cdylib. `Resource` impl that
  delegates to the loaded `invoke`. `RuinFn` that drops the
  `Library`. Tests against `wasm-loader.wat`-style inputs (a
  reference cdylib fixture is needed — small, but a real piece
  of work).
- **Week 4**: Hardening. Path validation, panic-wrap,
  per-plugin crash logging, plugin unload error surfaces.
  Examples + docs.

**3–4 weeks for one engineer.** The estimate assumes the
serialisation format choice is made in week 1 (it blocks the
buffer framing module).

For **both cdylib and WASM**:

- **Week 5–6**: WIT design. Minimal host interface: a clock
  source, a log sink, and access to the plugin's own capability
  set (read-only). WIT file + `wasmtime::Linker` impl.
- **Week 6–7**: WASM backend. `wasmtime::Engine` + `Store`
  configuration (fuel, epoch, memory cap). Module load +
  typed-function binding for `name` and `invoke`. `MintFn` body
  (same shape as cdylib's, different inner transport).
- **Week 7–8**: Cross-backend tests (a single plugin written
  twice — once as cdylib, once as WASM — must produce
  identical observable behaviour through the cap). Docs.

**6–8 weeks for one engineer, total.**

Components that show up in either estimate:

| Component                                | cdylib-only | both |
| ---------------------------------------- | ----------- | ---- |
| Manifest schema (`BackendDecl`)          | week 1      | week 1 |
| Loader dispatch skeleton                 | week 1      | week 1 |
| Buffer framing / serialisation           | week 2      | week 2 |
| cdylib backend                           | week 2–3    | week 2–3 |
| `MintFn` / `Resource` impl for cdylib    | week 3      | week 3 |
| Path validation + panic-wrap + docs      | week 4      | week 4 |
| WIT design                               | —           | week 5–6 |
| `wasmtime` engine / linker setup         | —           | week 6 |
| WASM backend                             | —           | week 6–7 |
| Cross-backend conformance tests + docs   | —           | week 7–8 |

The numbers are honest: the WIT design is its own piece of
work and is the single biggest source of schedule risk in the
"both" track. If WIT slips by a week, the WASM backend slips
with it.

## Open questions

1. **WIT / host import surface.** What does a WASM plugin see
   of the kernel? Minimum viable: a clock (`now()`), a log
   sink (`log(level, msg)`), and a read-only view of its own
   cap set. Anything more (file I/O, network, inter-plugin
   messaging) is a security decision in its own right and
   should be designed separately. Decide this before the WASM
   backend commit lands.
2. **cdylib ABI stability across Rust versions.** `extern "C"`
   is stable. Anything else crossing the boundary is not. The
   plugin ABI must be plain C types only, and the plugin must
   be built with the same Rust toolchain version as the host
   (we should pin this in the plugin author guide, and
   ideally emit a clear compile error if the version mismatches
   at load time). This is a documentation problem more than a
   code problem, but it is the kind of thing that bites six
   months after launch when someone tries to upgrade the host.
3. **Plugin crash semantics.** Three options:
   (a) Document that a panicking cdylib kills the kernel;
       reject such plugins at load time.
   (b) Spawn each plugin in a child process; defeats the
       purpose of cdylib.
   (c) Wrap every `invoke` in `std::panic::catch_unwind`,
       surface a `HandlerPanicked` error.
   Recommendation: (c), with a documented requirement that
   plugin code uses `panic = "unwind"` and that plugins built
   with `panic = "abort"` are rejected at load time. WASM does
   not need this; traps are recoverable.
4. **Hot reload.** Out of scope for this slice. The current
   `load_plugin_from_path` is a one-shot — there is no
   `unload` + `reload` path on the running cspace. Adding hot
   reload is its own follow-up and is not assumed by any code
   in this scope.
5. **Plugin signing / verification.** Also out of scope. The
   loader trusts whatever path it is handed. Adding a signature
   check is a separate concern that belongs at the deploy
   pipeline, not in the loader.
