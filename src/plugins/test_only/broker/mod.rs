//! Broker plugin — Phase 1 experimental inter-plugin delegation.
//!
//! `BrokerResource` holds an `Arc<Capability<CounterResource>>` and a
//! reference to the `CapabilitySpace` (so it can mint derived slots).
//!
//! This is the **first capability delegation that does not originate
//! from the host**: the host mints `slot:counter` once, the broker
//! borrows it, and *the broker itself* uses `cspace.restrict(...)` to
//! produce a `slot:counter_child` that is fully observed by `revoke`.
//!
//! The capability the broker exposes (`slot:broker`) takes:
//!
//! ```json
//! {"op": "delegate", "name": "counter_child", "ops": ["READ"]}
//! ```
//!
//! and returns the freshly-allocated slot id. Phase 1 keeps it
//! minimal: no quota math, no remote ids, just the kernel-level
//! restrict path running *inside a plugin*.

mod handler;
pub use handler::*;