//! RuleAgent — type-agnostic orchestrator that dispatches via the
//! capability contract, not via hardcoded resource types.
//!
//! ## Phase 3.4 (capability-native)
//!
//! The agent's *reachable set* — the set of capability handles
//! it can address — is derived from the **resolved binding
//! table**, not from a hardcoded slot list. Concretely:
//!
//! - The agent's `manifest.toml` declares `[[requires]]` entries
//!   listing which capabilities it depends on.
//! - The resolver (`kernel::resolver`) walks the dependency
//!   graph at boot and produces `ResolvedPlan::bindings`:
//!   `{ handle, provider, capability, contract }` for each
//!   consumer.
//! - `AgentResource::from_bindings` reads
//!   `plan.bindings[consumer_plugin_id]` and stores one
//!   `Reachable { handle, capability }` per binding. The
//!   `handle` is the local name (what the agent uses to
//!   dispatch); the `capability` is the cspace name (what
//!   `lookup_by_name` resolves against).
//! - At dispatch, the agent matches the input's `target` against
//!   reachable handles. If matched, it looks up the cap via
//!   `cspace.lookup_by_name(reachable.capability)`.
//!
//! This is the **same binary** that, given two different
//! binding tables, produces two different reachable worlds:
//!
//! ```text
//! Agent A  bindings: [{handle:"counter", capability:"counter_a"}]
//!   reachable = ["counter"]
//!   dispatch("counter", "read")      → ✓ (via lookup_by_name("counter_a"))
//!   dispatch("echo",   "execute")   → ✗ (not in reachable)
//!
//! Agent B  bindings: [{handle:"counter", capability:"counter"},
//!                     {handle:"echo",   capability:"echo"}]
//!   reachable = ["counter", "echo"]
//!   dispatch("counter", "read")      → ✓
//!   dispatch("echo",   "execute")    → ✓
//! ```
//!
//! Note the β.4 trick: `handle` and `capability` can differ.
//! Two agents with identical `handle` lists but distinct
//! `capability` names reach *different* caps (e.g. the same
//! `"counter"` handle bound to `"counter_a"` vs `"counter"`).
//! This is how P3.4 supports both reachability *and* authority
//! differences through the same constructor.
//!
//! ## Authority check (carried over from P5)
//!
//! Once a cap is reachable, the agent still inspects the
//! contract to translate the action verb into the right
//! `OperationRights` bit, and the kernel-level guard inside
//! `invoke_op_dyn` enforces `requested ⊆ held`. The agent does
//! not invent mappings — the cap's contract is the authoritative
//! vocabulary.

use std::sync::Arc;

use cordis::{plugin_with, Context, Injection, LogLevel, Plugin};
use serde_json::{json, Value};

use crate::capability::{CapabilityChunk, CapabilitySpace, OperationRights, Resource, Slot, SlotId};
use crate::plugins::agent::program::ProgramStep;
use tokio::sync::mpsc;
use crate::kernel::manifest::PluginId;
use crate::kernel::resolver::{ResolvedBinding, ResolvedPlan};

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

// Reachable moved to crate::capability in Phase 4 P4.1
// because Generator's HttpModel and any future consumer
// need it. The agent test surface still uses it; it just
// imports from `crate::capability` now.
pub use crate::capability::Reachable;

/// Holds the agent's identity, the reachable set derived from
/// its binding table, and a reference to the cspace so it can
/// resolve them. Built via [`AgentResource::from_bindings`].
///
/// Note: no `SlotId`s are stored. The agent never sees slot
/// identifiers — only the binding table's `handle` and
/// `capability` strings.
pub struct AgentResource {
    name: String,
    /// Reachable entries from the binding table, sorted by
    /// `handle` for stable error messages and stable dispatch
    /// when the input has multiple candidates (it never should
    /// — duplicate handles are an error at construction).
    reachable: Vec<Reachable>,
    cspace: CapabilitySpace,
}

impl AgentResource {
    /// Construct from a [`ResolvedPlan`]. The reachable set is
    /// the set of bindings for `consumer` in `plan.bindings`.
    /// If `consumer` has no bindings (no `[[requires]]`),
    /// reachable is empty and the agent rejects every dispatch.
    pub fn from_bindings(
        name: impl Into<String>,
        plan: &ResolvedPlan,
        consumer: &PluginId,
        cspace: CapabilitySpace,
    ) -> Self {
        let mut reachable: Vec<Reachable> = plan
            .bindings
            .get(consumer)
            .map(|bs| bs.iter().map(Reachable::from_binding).collect())
            .unwrap_or_default();
        reachable.sort_by(|a, b| a.handle.cmp(&b.handle));
        // C2: log the reachable set at construction so a
        // missing-binding situation (empty `[[requires]]` ⇒
        // empty reachable) is visible immediately. Empty
        // reachable means "every dispatch will fail" — should
        // be loud, not silent until first invoke.
        let name_string = name.into();
        eprintln!(
            "[agent] {n}: reachable=[{list}]",
            n = name_string,
            list = reachable
                .iter()
                .map(|r| format!("{}→{}", r.handle, r.capability))
                .collect::<Vec<_>>()
                .join(", ")
        );
        // Duplicate handles mean the resolver produced two
        // bindings with the same `name` for the same consumer.
        // That's a resolver bug — surface it loudly here rather
        // than silently picking one.
        debug_assert!(
            reachable.windows(2).all(|w| w[0].handle != w[1].handle),
            "duplicate handles in binding table for consumer {:?}: {:?}",
            consumer,
            reachable.iter().map(|r| &r.handle).collect::<Vec<_>>()
        );
        // Strip duplicates only in release builds so tests can
        // still construct the resource after the assert fires.
        reachable.dedup_by(|a, b| a.handle == b.handle);
        Self {
            name: name_string,
            reachable,
            cspace,
        }
    }

    /// Inspect the reachable set. Tests assert the binding table
    /// shape without going through dispatch.
    pub fn reachable(&self) -> &[Reachable] {
        &self.reachable
    }

    /// Look up a specific binding by handle. Returns the
    /// `ResolvedBinding` (handle, provider, capability, contract)
    /// if the handle is reachable, or `None` otherwise.
    pub fn binding_for<'a>(
        plan: &'a ResolvedPlan,
        consumer: &PluginId,
        handle: &str,
    ) -> Option<&'a ResolvedBinding> {
        plan.bindings
            .get(consumer)?
            .iter()
            .find(|b| b.handle == handle)
    }
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
                    "{}: input must contain {{\"target\": \"<handle>\", \"op\": \"...\"}}",
                    self.name
                )
            })?;
        let action = input
            .get("op")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                format!("{}.{target}: input must contain {{\"op\": \"<action-verb>\"}}", self.name)
            })?;

        // 2) Capability-native gate: the target handle must be
        //    in the agent's reachable set. This is the only
        //    place where "what can the agent do" is decided.
        let entry = self
            .reachable
            .iter()
            .find(|r| r.handle == target)
            .ok_or_else(|| {
                let names: Vec<&str> = self.reachable.iter().map(|r| r.handle.as_str()).collect();
                format!(
                    "{}: target \"{target}\" is not in the binding table; reachable = {names:?}",
                    self.name
                )
            })?;

        // 3) Resolve the capability at the slot by the
        //    binding's `capability` name. If the binding exists
        //    but the cap was never installed in cspace (e.g.
        //    the provider didn't activate), this returns None.
        let cap = self
            .cspace
            .lookup_by_name(&entry.capability)
            .ok_or_else(|| {
                format!(
                    "{}.{target}: reachable (handle={}, capability={}) but cap missing in cspace",
                    self.name, entry.handle, entry.capability
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
                    self.name,
                    cap.meta().authority.actions.iter().map(|a| &a.name).collect::<Vec<_>>()
                )
            })?;
        let requested_op = parse_operation(op_str).ok_or_else(|| {
            format!(
                "{}.{target}: authority lists \"{action}\" → \"{op_str}\", which is not a known operation",
                self.name
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
                self.name, requested_op
            ));
        }

        // 6) Invoke via the operation-aware path.
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

    /// Phase 4 P4.4 — streaming program interpreter.
    ///
    /// Input: `{"program": [ProgramStep, ...]}`.
    ///
    /// For each step, the agent emits a `step_start` event,
    /// then one of:
    ///
    /// - `step_ok`   — handle in reachable, slot in cspace, invoke ok
    /// - `step_skip` — handle not in reachable (env doesn't grant)
    /// - `step_deny` — handle in reachable, but cap's authority
    ///                 doesn't include the step's `op` (when set)
    /// - `step_fail` — invoke itself returned an Err
    ///
    /// The agent never aborts on a failed step — it records
    /// the outcome and proceeds to the next. This is the
    /// Phase 4 thesis property: capability failure is data,
    /// not crash. The final `done` event summarises counts.
    fn open(
        &self,
        input: Value,
    ) -> Result<mpsc::Receiver<CapabilityChunk>, String> {
        let program_val = input
            .get("program")
            .cloned()
            .ok_or_else(|| format!("{}: input must contain 'program'", self.name))?;
        let program: Vec<ProgramStep> = serde_json::from_value(program_val)
            .map_err(|e| format!("{}: program parse: {e}", self.name))?;

        let (tx, rx) = mpsc::channel(16);
        let name = self.name.clone();
        let reachable = self.reachable.clone();
        let cspace = self.cspace.clone();

        tokio::spawn(async move {
            run_program(name, reachable, cspace, program, tx).await;
        });
        Ok(rx)
    }
}

/// Outcome counters for one program run. Used to build the
/// final `done` event.
#[derive(Default)]
struct RunStats {
    ok: usize,
    skipped: usize,
    denied: usize,
    failed: usize,
}

/// The actual streaming interpreter. Pulled out of the impl
/// block so it can be tested as a plain function.
async fn run_program(
    name: String,
    reachable: Vec<Reachable>,
    cspace: CapabilitySpace,
    program: Vec<ProgramStep>,
    tx: mpsc::Sender<CapabilityChunk>,
) {
    let mut stats = RunStats::default();
    for (i, step) in program.iter().enumerate() {
        // step_start
        if tx
            .send(CapabilityChunk::Item(json!({
                "event":   "step_start",
                "index":   i,
                "handle":  step.handle,
                "op":      step.op,
            })))
            .await
            .is_err()
        {
            return;
        }

        // 1) Reachable check
        let entry = match reachable.iter().find(|r| r.handle == step.handle) {
            Some(e) => e,
            None => {
                emit_skip(&tx, i, &step.handle, "env doesn't grant this capability", &mut stats).await;
                continue;
            }
        };

        // 2) Slot check
        let cap = match cspace.lookup_by_name(&entry.capability) {
            Some(c) => c,
            None => {
                emit_skip(&tx, i, &step.handle, "reachable but cap missing in cspace", &mut stats).await;
                continue;
            }
        };

        // 3) Authority check (only if step specifies an op)
        if let Some(op_name) = &step.op {
            match cap.meta().authority.operation_for(op_name) {
                None => {
                    emit_event(&tx, json!({
                        "event": "step_deny",
                        "index": i,
                        "handle": step.handle,
                        "op": op_name,
                        "reason": "op not in cap's published authority",
                    }))
                    .await;
                    stats.denied += 1;
                    continue;
                }
                Some(_op_str) => {
                    // op_str is "READ"/"WRITE"/etc. Phase 4
                    // does not enforce the held-bits check
                    // here — that lives at the kernel level
                    // (`invoke_op_dyn`). If the cap holds the
                    // bit, the invoke will succeed; if not, the
                    // invoke returns Err and we record fail.
                }
            }
        }

        // 4) Dispatch via invoke_dyn.
        match cap.invoke_dyn(step.input.clone()) {
            Ok(output) => {
                emit_event(&tx, json!({
                    "event": "step_ok",
                    "index": i,
                    "handle": step.handle,
                    "output": output,
                }))
                .await;
                stats.ok += 1;
            }
            Err(e) => {
                emit_event(&tx, json!({
                    "event": "step_fail",
                    "index": i,
                    "handle": step.handle,
                    "error": e,
                }))
                .await;
                stats.failed += 1;
            }
        }
    }

    // done summary
    let _ = tx
        .send(CapabilityChunk::Item(json!({
            "event":   "done",
            "steps":   program.len(),
            "ok":      stats.ok,
            "skipped": stats.skipped,
            "denied":  stats.denied,
            "failed":  stats.failed,
        })))
        .await;
    let _ = tx.send(CapabilityChunk::Done).await;
}

async fn emit_event(tx: &mpsc::Sender<CapabilityChunk>, payload: Value) {
    let _ = tx.send(CapabilityChunk::Item(payload)).await;
}

async fn emit_skip(
    tx: &mpsc::Sender<CapabilityChunk>,
    index: usize,
    handle: &str,
    reason: &str,
    stats: &mut RunStats,
) {
    emit_event(
        tx,
        json!({
            "event":  "step_skip",
            "index":  index,
            "handle": handle,
            "reason": reason,
        }),
    )
    .await;
    stats.skipped += 1;
}

/// Backwards-compatible constructor used by tests that build
/// the reachable set from `(handle, SlotId)` pairs and want to
/// keep the slot-id path (β.4 uses this for "same reachability,
/// different authority").
///
/// Looks up each slot's *registered* name in cspace
/// (`cspace.name_for_slot`), which for derived caps (the result
/// of `restrict`/`grant`) is the new name (e.g. `"counter_a"`)
/// rather than the cap's own `meta.name` (which inherits from
/// the parent, e.g. `"counter"`). The agent then dispatches via
/// the binding-table path (handle matching, then
/// `lookup_by_name(capability)`).
///
/// **Prefer** [`AgentResource::from_bindings`] in new tests —
/// this helper just bridges the legacy slot-id style.
pub fn handler(
    name: impl Into<String>,
    slots: Vec<(String, SlotId)>,
    cspace: CapabilitySpace,
) -> Arc<AgentResource> {
    let mut entries: Vec<Reachable> = slots
        .iter()
        .map(|(handle, slot_id)| Reachable {
            handle: handle.clone(),
            capability: cspace
                .name_for_slot(*slot_id)
                .unwrap_or_else(|| format!("slot:{}", slot_id.raw())),
        })
        .collect();
    entries.sort_by(|a, b| a.handle.cmp(&b.handle));
    entries.dedup_by(|a, b| a.handle == b.handle);
    Arc::new(AgentResource {
        name: name.into(),
        reachable: entries,
        cspace,
    })
}

/// Capability-native constructor used by tests that have a
/// real `ResolvedPlan` (e.g. from `kernel::resolver::resolve`).
/// Wraps [`AgentResource::from_bindings`] and returns the
/// `Arc<AgentResource>` that `factory.mint::<AgentResource>`
/// expects.
pub fn handler_from_plan(
    name: impl Into<String>,
    plan: &ResolvedPlan,
    consumer: &PluginId,
    cspace: CapabilitySpace,
) -> Arc<AgentResource> {
    Arc::new(AgentResource::from_bindings(
        name,
        plan,
        consumer,
        cspace,
    ))
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
                ),
            );
            Ok(())
        },
    )
}