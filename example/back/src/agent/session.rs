//! Session-level machinery — the per-session event bus,
//! the global sessions table, and the bus-mint helper.
//!
//! Split out from `runtime.rs` so the broadcast/slot-id
//! plumbing has its own file.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use odyssey::capability::enforce::quota::CapabilityBudget;
use odyssey::capability::enforce::space::CapabilitySpace;
use odyssey::capability::handle::cap::Capability;
use odyssey::core::Resource;
use odyssey::core::clock::clock::SystemClock;
use odyssey::core::identity::ids::{CapabilityId, PluginId, SlotId};
use odyssey::core::identity::kind::CapKind;
use odyssey::core::meta::meta::CapabilityMeta;
use odyssey::core::rights::rights::{CapabilityRights, Rights};
use serde_json::Value;
use tokio::sync::broadcast;

use crate::agent::types::{AgentEvent, SessionId, Sessions};

/// Typed capability that publishes `AgentEvent`s to a
/// session's broadcast. The model plugins (and future
/// streaming tools) invoke this resource to push per-token
/// deltas, replacing the old cross-call into the runtime's
/// static `SESSIONS` table.
pub struct SessionEventBusResource {
    sender: broadcast::Sender<AgentEvent>,
}

impl Resource for SessionEventBusResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let kind = input
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| "session_event_bus: missing `kind`".to_string())?;
        let event = match kind {
            "llm_delta" => AgentEvent::LlmDelta(
                input
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "session_event_bus(llm_delta): missing `text`".to_string())?
                    .to_string(),
            ),
            "llm_reply_text" => AgentEvent::LlmReplyText(
                input
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "session_event_bus(llm_reply_text): missing `text`".to_string())?
                    .to_string(),
            ),
            "error" => AgentEvent::Error(
                input
                    .get("message")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "session_event_bus(error): missing `message`".to_string())?
                    .to_string(),
            ),
            // `tool_call` and `tool_result` are produced by
            // the agent itself (not the LLM), so the bus
            // doesn't need to handle them here.
            other => {
                return Err(format!(
                    "session_event_bus: unhandled kind `{other}` (LLM should only emit llm_delta / llm_reply_text / error)"
                ));
            }
        };
        // Best-effort: drop events with no listener.
        let _ = self.sender.send(event);
        Ok(Value::Null)
    }
}

/// Mint a fresh `CapabilityId` for a session event bus.
fn next_bus_capability_id() -> CapabilityId {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    CapabilityId(NEXT.fetch_add(1, Ordering::Relaxed))
}

/// Mint a `SessionEventBusResource` into `cspace` and return
/// its slot id. Callers use the slot id directly, never the
/// opaque name.
pub fn mint_session_event_bus(
    cspace: &CapabilitySpace,
    sender: broadcast::Sender<AgentEvent>,
) -> SlotId {
    let slot_id = cspace.allocate();
    let meta = CapabilityMeta {
        id: next_bus_capability_id(),
        name: format!("__session_event_bus_{}", slot_id.raw()),
        namespace: "agent".into(),
        contract_name: "session_event_bus".into(),
        plugin: PluginId {
            name: "agent".into(),
            version: "0.1.0".into(),
        },
        kind: CapKind::Sync,
        timeout_ms: 1000,
        quota: Default::default(),
        tool_schema: None,
    };
    let rights = CapabilityRights {
        operations: Rights::INVOKE,
        timeout_ms: 1000,
    };
    let budget = CapabilityBudget::new(1000);
    let cap = Capability::new(
        meta,
        Arc::new(SessionEventBusResource { sender }),
        budget,
        rights,
        CapKind::Sync,
        Arc::new(SystemClock),
    );
    cspace.install(slot_id, Arc::new(cap));
    slot_id
}

/// Lazily-initialised global sessions table.
pub fn global_sessions() -> Sessions {
    static SESSIONS: OnceLock<Sessions> = OnceLock::new();
    SESSIONS
        .get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
        .clone()
}

/// Subscribe to a session's event broadcast by id.
pub fn subscribe_session_broadcast(session_id: &str) -> Option<broadcast::Receiver<AgentEvent>> {
    let sessions_arc = global_sessions();
    let sessions = sessions_arc.lock().expect("sessions poisoned");
    sessions
        .get(&SessionId(session_id.to_string()))
        .map(|s| s.sender.subscribe())
}
