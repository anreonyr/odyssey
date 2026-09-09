# echo-cdylib

Workspace member reserved for the cdylib loader path. The C-ABI
`EchoExports` / `CAbiHandler` types live in the parent crate's
`libloading`-based loader (currently unwired; see `main.rs` phase 3
`build_sync_handler`).

To wire this in:

1. Implement `ECHO_EXPORTS` here (an `EchoExports` struct matching the
   host's `ECHO_ABI_VERSION = 1`).
2. Build: `cargo build -p echo-cdylib` — produces
   `target/debug/libecho_cdylib.so`.
3. Add an `echo-cdylib` handler branch to `main.rs::build_sync_handler`
   that calls `loader::load_echo_cdylib_handler(manifest)` and wraps the
   returned `Arc<dyn SyncInvoke>`.

Until then, the `echo-cdylib` manifest loads but its capability is
skipped at mint time:

```
[skip] echo-cdylib: deferred
```