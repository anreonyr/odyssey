//! Echo WASM — deferred. The manifest (`echo_wasm.toml`) references
//! `echo.wat` in this directory but the wasmtime loader is not wired;
//! main.rs prints a `[skip]` and the capability is never minted.