//! RuleAgent — type-agnostic orchestrator that dispatches via the
//! capability contract, not via hardcoded resource types.
//!
//! The agent has no knowledge of any specific resource. For each
//! target slot it addresses, it asks the held capability's contract
//! what `op` strings the cap accepts and which `OperationRights`
//! bit each one requires. The kernel still enforces the bit at the
//! `invoke_op_dyn` boundary, so the agent's only job is to publish
//! the right bit to the kernel — not to make the authority decision.
//!
//! This is the same program that, given two different capability
//! environments (and two different contracts), produces two
//! different reachable worlds.

use std::sync::Arc;

use cordis::{plugin_with, Context, Injection, LogLevel, Plugin};
use serde_json::{json, Value};

use crate::capability::{CapabilitySpace, OperationRights, Resource, Slot, SlotId};

/// Parse a contract-published operation string ("READ", "WRITE",
/// "EXECUTE", "ADMIN") into the corresponding bit. Returns `None`
/// for any other value — the cap author is responsible for
/// publishing only valid strings in the contract.
fn parse_operation(s: &str) -> Option<OperationRights> {
    Some(match s {
        "READ" => OperationRights::READ,
        "WRITE" => OperationRights::WRITE,
        "EXECUTE" => OperationRights::EXECUTE,
        "ADMIN" => OperationRights::ADMIN,
        _ => return None,
    })
}

/// Holds the (name, slot) pairs the agent can address, plus the
/// cspace reference for lookups. Built by the host and passed to
/// the handler.
pub struct AgentResource {
    name: String,
    /// Addressable slots: name → slot id.
    slots: Vec<(String, SlotId)>,
    cspace: CapabilitySpace,
}

impl Resource for AgentResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        // 1) Resolve which slot to address. The agent uses the
        //    input's `target` field (a slot name) for explicit
        //    dispatch.
        let target = input
            .get("target")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                format!(
                    "{}: input must contain {{\"target\": \"<slot-name>\", \"op\": \"...\"}}",
                    self.name
                )
            })?;
        let action = input
            .get("op")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                format!("{}.{target}: input must contain {{\"op\": \"<action-verb>\"}}", self.name)
            })?;

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

        // 3) Look up the operation bit the contract says the caller
        //    must hold for this action. The cap's contract is the
        //    authoritative vocabulary — the agent does not invent
        //    mappings.
        let op_str = cap
            .meta()
            .contract
            .operation_for(action)
            .ok_or_else(|| {
                format!(
                    "{}.{target}: action \"{action}\" not in cap's published contract (actions = {:?})",
                    self.name,
                    cap.meta().contract.actions.iter().map(|a| &a.name).collect::<Vec<_>>()
                )
            })?;
        let requested_op = parse_operation(op_str).ok_or_else(|| {
            format!(
                "{}.{target}: contract lists \"{action}\" → \"{op_str}\", which is not a known operation",
                self.name
            )
        })?;

        // 4) Inspect the cap's operations via the erased view
        //    (`AnyCapability::operations`). The kernel-level guard
        //    inside `invoke_dyn` will also reject if the bit isn't
        //    held, but checking here first gives a clearer error
        //    message ("not in held ops") rather than the generic
        //    "operation denied" from inside the resource.
        let held_ops = cap.operations();
        if !held_ops.contains(requested_op) {
            return Err(format!(
                "{}.{target}: action \"{action}\" needs {:?} which is not in held ops {held_ops:?}",
                self.name, requested_op
            ));
        }

        // 5) Invoke via the operation-aware path. `invoke_op_dyn`
        //    lets the kernel enforce `requested_op ⊆ held_ops` so
        //    the Agent doesn't need any per-resource awareness.
        let result = cap
            .invoke_op_dyn(requested_op, json!({ "op": action }))
            .map_err(|e| format!("{}.{target}: {e}", self.name))?;

        Ok(json!({
            "agent": self.name,
            "target": target,
            "action": action,
            "operation": format!("{requested_op:?}"),
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
                format!(
                    "agent plugin: slot={} cap_id={}",
                    slot.id().raw(),
                    slot.capability()
                        .map(|c| c.id().to_string())
                        .unwrap_or_else(|| "(empty)".to_string()),
                )
                .into(),
            );
            Ok(())
        },
    )
}