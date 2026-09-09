//! RuleAgent plugin — Phase 2 P5: same program, different authority.
//!
//! The RuleAgent has **no hardcoded knowledge of any specific
//! resource type**. It receives, at activation time, a list of
//! `(name, SlotId)` pairs it can address, plus the `CapabilitySpace`
//! so it can resolve them. On invoke, it inspects the input,
//! identifies which slot to address, and dispatches via
//! `cspace.lookup_erased(slot)` + `cap.invoke_dyn(...)`.
//!
//! This is the **same program** that, given two different capability
//! environments, produces two different behaviors:
//!
//! ```text
//! Agent A
//! ├── slot:counter  ops = READ
//! └── slot:echo     ops = EXECUTE
//!
//! Agent A: read ✓, increment ✗, reset ✗
//!
//! Agent B
//! ├── slot:counter  ops = READ | WRITE
//! └── slot:echo     ops = EXECUTE
//!
//! Agent B: read ✓, increment ✓, reset ✗
//! ```
//!
//! The agent itself doesn't decide whether increment is allowed —
//! `Capability::invoke_op` does, via the kernel-level guard.
//! Different capabilities, different reachable world.

mod handler;
pub use handler::*;