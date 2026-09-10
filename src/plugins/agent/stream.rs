//! Streaming dispatch for the RuleAgent.
//!
//! Phase 4 P4.4 — the streaming `Resource::open` body. Input
//! is `{"program": [ProgramStep, ...]}`; for each step the
//! agent emits a `step_start` event and then one of
//! `step_ok` / `step_skip` / `step_deny` / `step_fail`. The
//! final `done` event summarises counts.
//!
//! Phase 6 split: this file owns:
//!
//! - `open_streaming` — the `Resource::open` body. Forwards to
//!   `run_program` via `tokio::spawn`.
//! - `run_program` — the per-step interpreter. Pulled out of
//!   the impl block so it can be tested as a plain function.
//! - `RunStats` — outcome counters for the final `done` event.
//! - `emit_event` / `emit_skip` — per-event emission helpers.
//! - `panic_payload_to_str` lives in `panic.rs` (shared
//!   formatter used by the panic-catch arms below).
//!
//! Phase 4 review-loop behaviours preserved verbatim:
//!
//! - **P0-c** — `tokio::task::yield_now` after `step_start`
//!   so a sibling task that wants to revoke (or mutate any
//!   other cspace state) gets a chance to run before the
//!   agent looks up the cap. Without this, a fully
//!   synchronous program on a 16-buffer mpsc would complete
//!   all steps before the test task wakes up to revoke,
//!   defeating mid-flight tests.
//! - **P0-a** — if `parse_operation` returns `None` for the
//!   authority-published string, emit `step_deny` rather
//!   than silently falling through to `invoke_dyn` with
//!   EXECUTE (the previous behaviour was a true auth bypass).
//! - **P4.4 thesis** — capability failure is data, not crash.
//!   The agent never aborts on a failed step. `step_fail` is
//!   emitted and the loop proceeds to the next step.
//! - **Panic containment** — `std::panic::catch_unwind` around
//!   the dispatch so a handler-side panic surfaces as a
//!   `step_fail` event instead of tearing down the spawned
//!   task.

use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::kernel::{CapabilityChunk, CapabilitySpace, OperationRights};
use crate::plugins::agent::handler::{parse_operation, AgentResource, Reachable};
use crate::plugins::agent::panic::panic_payload_to_str;
use crate::plugins::agent::program::ProgramStep;

/// The `Resource::open` body. Parses the program out of the
/// input, spawns `run_program` on the tokio runtime, and returns
/// the receiver half so the caller can stream events.
///
/// Kept as a free function (rather than an inherent method on
/// `AgentResource`) so `dispatch.rs` can forward to it without
/// having to put the entire interpreter here. The
/// `impl Resource for AgentResource` block in `dispatch.rs`
/// is the trait-visible entry point.
pub fn open_streaming(
    agent: &AgentResource,
    input: Value,
) -> Result<mpsc::Receiver<CapabilityChunk>, String> {
    let program_val = input
        .get("program")
        .cloned()
        .ok_or_else(|| format!("{}: input must contain 'program'", agent.name()))?;
    let program: Vec<ProgramStep> = serde_json::from_value(program_val)
        .map_err(|e| format!("{}: program parse: {e}", agent.name()))?;

    let (tx, rx) = mpsc::channel(16);
    let name = agent.name().to_string();
    let reachable = agent.reachable_vec();
    let cspace = agent.cspace().clone();

    tokio::spawn(async move {
        run_program(name, reachable, cspace, program, tx).await;
    });
    Ok(rx)
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
/// block so it can be tested as a plain function if we ever
/// want to (the existing tests drive it through `open`).
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

        // P0-c (Phase 4 review loop): yield once after
        // step_start so a sibling task that wants to revoke
        // gets a chance to run before we look up the cap.
        tokio::task::yield_now().await;

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

        // 3) Authority check (only if step specifies an op).
        let mut requested_op: Option<OperationRights> = None;
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
                Some(op_str) => {
                    // P0-a: parse_operation returning None
                    // means the authority published a string
                    // that does not map to a known bit. The
                    // sync path treats this as a deny; the
                    // streaming path used to silently fall
                    // back to EXECUTE (auth bypass). Mirror
                    // the sync path: emit step_deny, continue.
                    requested_op = parse_operation(op_str);
                    if requested_op.is_none() {
                        emit_event(&tx, json!({
                            "event": "step_deny",
                            "index": i,
                            "handle": step.handle,
                            "op": op_name,
                            "reason": format!("{op_str}: op_str not a known operation bit"),
                        }))
                        .await;
                        stats.denied += 1;
                        continue;
                    }
                }
            }
        }

        // 4) Dispatch. Panic containment wraps the call so a
        //    handler-side panic surfaces as `step_fail` rather
        //    than unwinding through the `tokio::spawn`
        //    boundary.
        let dispatch_result = match requested_op {
            Some(op) => match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                cap.invoke_op_dyn(op, step.input.clone())
            })) {
                Ok(r) => r,
                Err(panic_payload) => Err(format!(
                    "{}.{}: handler panicked: {}",
                    name,
                    step.handle,
                    panic_payload_to_str(&panic_payload)
                )),
            },
            None => match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                cap.invoke_dyn(step.input.clone())
            })) {
                Ok(r) => r,
                Err(panic_payload) => Err(format!(
                    "{}.{}: handler panicked: {}",
                    name,
                    step.handle,
                    panic_payload_to_str(&panic_payload)
                )),
            },
        };
        match dispatch_result {
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
                // If the kernel refused due to missing bits,
                // surface as step_deny (P4.6 — capability
                // attenuation). Otherwise it's a generic
                // step_fail.
                if e.contains("operation denied") {
                    emit_event(&tx, json!({
                        "event": "step_deny",
                        "index": i,
                        "handle": step.handle,
                        "op": step.op,
                        "reason": e,
                    }))
                    .await;
                    stats.denied += 1;
                } else {
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
