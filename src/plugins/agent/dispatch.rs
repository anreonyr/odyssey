//! Sync dispatch for the RuleAgent.
//!
//! `impl Resource for AgentResource { fn invoke }` — the
//! request/response path. The input is
//! `{"target": "<handle>", "op": "<action-verb>"}`; the agent
//! walks reachable → cspace → authority → ops-held and dispatches
//! via `cap.invoke_op_dyn`.
//!
//! Phase 6 split: the sync path lives here so the streaming
//! path's `run_program` interpreter in `stream.rs` can be
//! reviewed independently. Both paths share `parse_operation`
//! from `handler.rs` and the agent's own `reachable`/`cspace`
//! accessors.

use serde_json::{json, Value};

use crate::kernel::{CapabilityChunk, Resource};
use crate::plugins::agent::handler::{parse_operation, AgentResource};

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
                    "{}: input must contain {{\"target\": \"<handle>\", \"op\": \"...\"}}",
                    self.name()
                )
            })?;
        let action = input
            .get("op")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                format!(
                    "{}.{target}: input must contain {{\"op\": \"<action-verb>\"}}",
                    self.name()
                )
            })?;

        // 2) Capability-native gate: the target handle must be
        //    in the agent's reachable set. This is the only
        //    place where "what can the agent do" is decided.
        let entry = self
            .reachable()
            .iter()
            .find(|r| r.handle == target)
            .ok_or_else(|| {
                let names: Vec<&str> =
                    self.reachable().iter().map(|r| r.handle.as_str()).collect();
                format!(
                    "{}: target \"{target}\" is not in the binding table; reachable = {names:?}",
                    self.name()
                )
            })?;

        // 3) Resolve the capability at the slot by the
        //    binding's `capability` name. If the binding exists
        //    but the cap was never installed in cspace (e.g.
        //    the provider didn't activate), this returns None.
        let cap = self
            .cspace()
            .lookup_by_name(&entry.capability)
            .ok_or_else(|| {
                format!(
                    "{}.{target}: reachable (handle={}, capability={}) but cap missing in cspace",
                    self.name(),
                    entry.handle,
                    entry.capability
                )
            })?;

        // 4) Look up the operation bit the **authority contract** says
        //    the caller must hold for this action. Phase 3 P3.2
        //    split the original `CapabilityContract` into
        //    `AuthorityContract` (action → op map; load-bearing
        //    here) and `Protocol` (schemas, transport; pure
        //    metadata). The agent only reads authority.
        let op_str = cap
            .meta()
            .authority
            .operation_for(action)
            .ok_or_else(|| {
                format!(
                    "{}.{target}: action \"{action}\" not in cap's published authority (actions = {:?})",
                    self.name(),
                    cap.meta().authority.actions.iter().map(|a| &a.name).collect::<Vec<_>>()
                )
            })?;
        let requested_op = parse_operation(op_str).ok_or_else(|| {
            format!(
                "{}.{target}: authority lists \"{action}\" → \"{op_str}\", which is not a known operation",
                self.name()
            )
        })?;

        // 5) Inspect the cap's operations. The kernel-level
        //    guard inside `invoke_op_dyn` will also reject if
        //    the bit isn't held; the check here gives a clearer
        //    error message ("not in held ops") rather than the
        //    generic "operation denied" from inside the resource.
        let held_ops = cap.operations();
        if !held_ops.contains(requested_op) {
            return Err(format!(
                "{}.{target}: action \"{action}\" needs {:?} which is not in held ops {held_ops:?}",
                self.name(),
                requested_op
            ));
        }

        // 6) Invoke via the operation-aware path.
        let result = cap
            .invoke_op_dyn(requested_op, json!({ "op": action }))
            .map_err(|e| format!("{}.{target}: {e}", self.name()))?;

        Ok(json!({
            "agent": self.name(),
            "target": target,
            "action": action,
            "operation": format!("{requested_op:?}"),
            "operations": format!("{held_ops:?}"),
            "result": result,
            "meta_name": cap.meta().name.clone(),
        }))
    }

    /// Phase 4 P4.4 — streaming program interpreter. The body
    /// lives in `stream.rs`; the signature stays here so the
    /// `Resource` trait sees both methods as one impl block.
    fn open(
        &self,
        input: Value,
    ) -> Result<tokio::sync::mpsc::Receiver<CapabilityChunk>, String> {
        // Forward to the streaming path. `stream.rs` provides
        // `super::stream::open_streaming` so this impl block
        // stays small and the chunk-forwarding / panic-recovery
        // code is reviewed in isolation.
        super::stream::open_streaming(self, input)
    }
}
