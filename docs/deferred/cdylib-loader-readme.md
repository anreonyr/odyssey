//! Echo cdylib — deferred, and now further from landing than when it
//! was written.
//!
//! The design sketch is `cdylib-loader.toml` in this directory. Do not
//! read it as a manifest the current code accepts: it predates Phase 8
//! and Phase 9 and every section below has since changed.
//!
//! What the sketch assumes, and what is true now:
//!
//! - `[isolate]` — `IsolationMode` today has exactly one variant,
//!   `InProc`, and `ManifestBuilder::build` hardcodes it. Nothing
//!   parses an isolation table.
//! - `[source]` — deleted. There is no source-locating concept left in
//!   the manifest type.
//! - `streaming = false` — replaced by `kind = CapKind` (`"sync"` or
//!   `"stream"`). A TOML carrying `streaming` no longer parses as a
//!   capability decl.
//! - `plugins/echo.toml`, referenced in the header comment as the
//!   in-process counterpart — `src/plugins/` was removed in Phase 8.
//!   The in-process echo is now `builtins/src/echo.rs`.
//! - The TOML loader itself (`from_toml_str` / `from_path` /
//!   `validate`) was deleted in Phase 9. No manifest in this repo is
//!   read from TOML any more; the addressable entry point is
//!   `builtins/src/echo.rs` plus the cdylib adapter this file defers.
//!
//! Landing it therefore means: re-cut the manifest shape against the
//! current `CapabilityDecl` / `IsolationMode`, restore a loader, and
//! wire a `MintFn` for the external module. Only the ABI intent below
//! still carries over — see `wasm-loader.wat`, which documents the C
//! ABI (`name` / `invoke`, exported `memory`, return codes 0/1/2) that
//! a cdylib would be expected to implement.
