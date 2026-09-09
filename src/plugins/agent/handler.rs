//! RuleAgent — type-agnostic orchestrator that dispatches via the
//! capability graph, not via hardcoded resource types.

use std::sync::Arc;

use cordis::{plugin_with, Context, Injection, LogLevel, Plugin};
use serde_json::{json, Value};

use crate::capability::{CapabilitySpace, OperationRights, Resource, Slot, SlotId};

/// Holds the (name, slot) pairs the agent can address, plus the
/// cspace reference for lookups. Built by the host (or a lab) and
/// passed to the handler.
pub struct AgentResource {
    name: String,
    /// Addressable slots: name → slot id.
    slots: Vec<(String, SlotId)>,
    cspace: CapabilitySpace,
}

impl Resource for AgentResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        // 1) Resolve which slot to address. The agent uses the input's
        //    `target` field (a slot name) for explicit dispatch. If
        //    absent, the agent dispatches to its first slot.
        let target = input
            .get("target")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!(
                "{}: input must contain {{\"target\": \"<slot-name>\", \"op\": \"...\"}}",
                self.name
            ))?;
        let action = input
            .get("op")
            .and_then(|v| v.as_str())
            .unwrap_or("invoke");

        let slot_id = self
            .slots
            .iter()
            .find(|(n, _)| n == target)
            .map(|(_, s)| *s)
            .ok_or_else(|| {
                format!(
                    "{}: target \"{target}\" not in slot table; available = {:?}",
                    self.name,
                    self.slots.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>()
                )
            })?;

        // 2) Resolve the capability at the slot. If the slot is
        //    revoked, this returns None — and the agent honestly
        //    reports "no authority" rather than panicking.
        let cap = self
            .cspace
            .lookup_erased(slot_id)
            .ok_or_else(|| format!("{}.{target}: slot revoked", self.name))?;

        // 3) Determine the operation bit from the action verb. This
        //    is the agent's *policy*; the kernel will reject anything
        //    the cap doesn't actually hold.
        let requested_op = match action {
            "read" | "get" | "fetch" => OperationRights::READ,
            "write" | "set" | "update" | "increment" => OperationRights::WRITE,
            "execute" | "run" | "invoke" | "send" => OperationRights::EXECUTE,
            "admin" | "drop" | "revoke" | "reset" => OperationRights::ADMIN,
            other => {
                return Err(format!(
                    "{}.{target}: unknown action \"{other}\"",
                    self.name
                ));
            }
        };

        // 4) Inspect the cap's operations via the erased view
        //    (`AnyCapability::operations`). The kernel-level guard
        //    inside `invoke_dyn` will also reject if the bit isn't
        //    held, but checking here first gives a clearer error
        //    message ("not in held ops") rather than the generic
        //    "operation denied" from inside the resource.
        let held_ops = cap.operations();
        if !held_ops.contains(requested_op) {
            return Err(format!(
                "{}.{target}: operation \"{action}\" ({requested_op:?}) not in held ops {held_ops:?}",
                self.name
            ));
        }

        // 5) Invoke via the operation-aware path. `invoke_op_dyn` lets the
        //    kernel enforce `requested_op ⊆ held_ops` so the Agent
        //    doesn't need any per-resource awareness.
        let payload = json!({ "op": action });
        let result = cap
            .invoke_op_dyn(requested_op, payload)
            .map_err(|e| format!("{}.{target}: {e}", self.name))?;

        Ok(json!({
            "agent": self.name,
            "target": target,
            "action": action,
            "operations": format!("{held_ops:?}"),
            "result": result,
            "meta_name": cap.meta().name.clone(),
        }))
    }
}

pub fn handler(
    name: impl Into<String>,
    slots: Vec<(String, SlotId)>,
    cspace: CapabilitySpace,
) -> Arc<AgentResource> {
    Arc::new(AgentResource {
        name: name.into(),
        slots,
        cspace,
    })
}

pub fn agent_plugin() -> Arc<dyn Plugin> {
    plugin_with(
        "agent",
        vec![Injection::from("slot:agent")],
        |ctx: Context, _cfg: ()| async move {
            let slot: Arc<Slot<AgentResource>> = ctx.require("slot:agent")?;
            ctx.logger().log(
                LogLevel::Info,
                format!("agent plugin: slot={} cap_id={}",
                    slot.id().raw(),
                    slot.capability().map(|c| c.id().to_string())
                        .unwrap_or_else(|| "(empty)".to_string()),
                ).into(),
            );
            Ok(())
        },
    )
}