//! Counter plugin — Phase 1 experimental `CounterResource`.
//!
//! The body is a single shared integer behind a `Mutex`. Three
//! operations, each gated by an `OperationRights` bit:
//!
//! | operation | op bit  | effect                 |
//! |-----------|---------|------------------------|
//! | `read`    | READ    | returns current value  |
//! | `increment`| WRITE  | adds 1, returns new    |
//! | `reset`   | ADMIN   | sets to 0, returns 0   |
//!
//! `Capability::invoke_op` is what enforces the bits — without the bit,
//! the handler never runs. This is the single resource the four labs
//! exercise to make `restrict` / `revoke` / `transfer` observable.

mod handler;
pub use handler::*;