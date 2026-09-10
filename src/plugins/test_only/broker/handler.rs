//! Broker resource — holds a capability and mints derived slots.
//!
//! The broker demonstrates the first "plugin-level" delegation path:
//! the host mints the root capability and injects it; the broker then
//! uses `cspace.restrict` to derive a more limited view of the
//! capability for some downstream plugin, without going back to the
//! host. Capability authority now flows through a chain rather than
//! fanning out from one root.
//!
//! Phase 1 keeps this intentionally minimal: no quotas, no nested
//! brokers. The interesting property is that revocation still works
//! because the broker only ever holds a `Capability<R>` reference;
//! `cspace.revoke(slot)` works on the slot, not the reference.

use std::sync::Arc;

use cordis::{plugin_with, Context, Injection, LogLevel, Plugin};
use serde_json::{json, Value};

use crate::kernel::{
    Capability, CapabilityError, CapabilityRights, CapabilitySpace, OperationRights, Resource,
    Slot, SlotId,
};
use crate::plugins::test_only::counter::CounterResource;

/// Operations the broker accepts on its own capability:
///
/// - `describe` → metadata about the held counter capability.
/// - `delegate` → mint a derived slot under `name` with the given
///   `ops` array, returning the new slot id. Returns an error if the
///   requested ops are not a subset of what the broker itself holds
///   (the kernel's attenuation rule, enforced inside `cspace.restrict`).
pub struct BrokerResource {
    counter: Arc<Capability<CounterResource>>,
    /// Slot id where the held counter capability lives. Needed so the
    /// broker can call `cspace.restrict(source, ...)` — the kernel
    /// verifies attenuation against the *slot's* held rights, not
    /// against the `Capability` value the broker happens to hold.
    counter_slot: SlotId,
    cspace: CapabilitySpace,
}

impl Resource for BrokerResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let op = input
            .get("op")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "broker: input must contain {\"op\": ...}".to_string())?;
        match op {
            "describe" => Ok(json!({
                "held_capability": self.counter.id().to_string(),
                "held_name":       self.counter.name(),
                "held_ops":        format!("{:?}", self.counter.operations()),
                "held_timeout_ms": self.counter.rights().timeout_ms,
                "source_slot":     self.counter_slot.to_string(),
            })),
            "delegate" => {
                let name = input
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "broker: delegate requires {name: \"...\"}".to_string())?
                    .to_string();
                let ops_arr = input
                    .get("ops")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| "broker: delegate requires {ops: [...]}".to_string())?;
                let mut requested = OperationRights::empty();
                for o in ops_arr {
                    let bit = o
                        .as_str()
                        .ok_or_else(|| "broker: ops must be strings".to_string())?;
                    requested |= match bit {
                        "READ"    => OperationRights::READ,
                        "WRITE"   => OperationRights::WRITE,
                        "EXECUTE" => OperationRights::EXECUTE,
                        "ADMIN"   => OperationRights::ADMIN,
                        other => return Err(format!("broker: unknown op \"{other}\"")),
                    };
                }
                // Kernel-level check: cspace.restrict enforces that
                // requested ⊆ held-at-slot. The broker does not need
                // to do its own subset check — failing here gives the
                // same answer either way, and the kernel is the
                // authority of record.
                let new_slot = self
                    .cspace
                    .restrict::<CounterResource>(
                        self.counter_slot,
                        CapabilityRights {
                            operations: requested,
                            timeout_ms: self.counter.rights().timeout_ms,
                        },
                        name,
                    )
                    .map_err(|e: CapabilityError| format!("broker: {e}"))?;
                Ok(json!({ "slot": new_slot.to_string() }))
            }
            other => Err(format!("broker: unknown op \"{other}\"")),
        }
    }
}

pub fn handler(
    counter: Arc<Capability<CounterResource>>,
    counter_slot: SlotId,
    cspace: CapabilitySpace,
) -> Arc<BrokerResource> {
    Arc::new(BrokerResource {
        counter,
        counter_slot,
        cspace,
    })
}

pub fn broker_plugin() -> Arc<dyn Plugin> {
    plugin_with(
        "broker",
        vec![
            Injection::from("slot:counter"),
            Injection::from("slot:broker"),
        ],
        |ctx: Context, _cfg: ()| async move {
            let counter_slot: Arc<Slot<CounterResource>> = ctx.require("slot:counter")?;
            let broker_slot: Arc<Slot<BrokerResource>> = ctx.require("slot:broker")?;
            ctx.logger().log(
                LogLevel::Info,
                format!(
                    "broker plugin: holds counter slot={} (cap={}); broker slot={}",
                    counter_slot.id().raw(),
                    counter_slot
                        .capability()
                        .map(|c| c.id().to_string())
                        .unwrap_or_else(|| "(empty)".to_string()),
                    broker_slot.id().raw(),
                ),
            );
            Ok(())
        },
    )
}