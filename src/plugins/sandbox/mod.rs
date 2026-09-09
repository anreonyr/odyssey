//! Sandbox plugin — `SandboxResource: Resource`. Sync only.
//!
//! Reads a WAT file and runs it in a wasmtime instance with a fuel
//! budget. Sample program lives in `sandbox_programs/hello.wat`.

mod handler;
pub use handler::*;
