//! Echo WASM — deferred, and now further from landing than when it was
//! written.
//!
//! The design sketch is `wasm-loader.toml` in this directory, with the
//! module source at `wasm-loader.wat`. Do not read the TOML as a
//! manifest the current code accepts: it predates Phase 8 and Phase 9.
//!
//! Stale sections, same set as the cdylib sketch:
//!
//! - `[isolate]` — `IsolationMode` today has exactly one variant,
//!   `InProc`; nothing parses an isolation table. A WASM loader would
//!   add the variant and the parsing.
//! - `[source]` — deleted; no source-locating concept remains.
//! - `streaming = false` — replaced by `kind = CapKind` (`"sync"` /
//!   `"stream"`).
//! - The TOML loader was deleted in Phase 9, so the `.toml` here is
//!   documentation of intent, not input to anything.
//!
//! `wasm-loader.wat` is still the useful half: it is a real, valid
//! module that pins the host ABI — exported `memory`, `name(out_buf,
//! cap, out_len)` writing the plugin name, and `invoke(in_buf, in_len,
//! out_buf, out_cap, out_len) -> i32` with 0 = ok, 1 = buffer too
//! small, 2 = invalid input. A loader must match that contract; the
//! sketch's `echo.wat` filename in the TOML header is wrong today (the
//! file is `wasm-loader.wat`).
//!
//! `src/core/manifest/manifest.rs` still points here twice in its
//! module docs as the place where WASM / cdylib designs are tracked.
//! Those pointers remain valid — the designs are tracked, not
//! implemented.
