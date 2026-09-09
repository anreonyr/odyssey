//! Echo cdylib — deferred. The manifest (`echo_cdylib.toml`) is loaded
//! at boot, but the loader is not wired; main.rs prints a `[skip]` and
//! the capability is never minted. See `echo-cdylib/README.md` for
//! the wiring plan.