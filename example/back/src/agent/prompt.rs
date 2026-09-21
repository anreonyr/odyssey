//! Prompt construction + LLM reply parsing — the
//! "talking to the model" helpers. Pulled out from
//! `runtime.rs` so the agent's state machine (start /
//! advance / cancel) doesn't have to carry these strings
//! inline.

use odyssey::capability::enforce::space::CapabilitySpace;
use serde_json::{Value, json};

use crate::agent::types::{
    AgentError, FinalAnswer, FinalReason, History, Session, SessionStatus, Step, ToolInvocation,
    ToolResult,
};
use odyssey::core::rights::rights::Rights;

pub fn build_system_prompt(first_action: bool, allowed_tools: &[String]) -> String {
    let tool_list = if allowed_tools.is_empty() {
        "(no tools available)".to_string()
    } else {
        allowed_tools.join(", ")
    };
    let mut s = String::new();
    s.push_str("You are an agent inside the odyssey kernel. ");
    s.push_str("Available tools: ");
    s.push_str(&tool_list);
    s.push_str(". ");
    if first_action {
        s.push_str("__AGENT_FIRST_ACTION__: this is the first turn; you should call a tool. ");
    } else {
        s.push_str("__AGENT_FINAL_AFTER_TOOL__: the previous tool call returned; you should produce a final answer now. ");
    }
    s.push_str("Reply with JSON: {\"tool_call\":{\"tool\":\"<name>\",\"args\":<json>}} to call a tool, or {\"final\":<value>} to finish. Plain text is treated as a final answer.");
    s
}

pub fn build_prompt(session: &Session, observation: &crate::agent::types::Observation) -> String {
    let mut s = String::new();
    s.push_str("Goal: ");
    s.push_str(&session.goal);
    s.push_str("\n\nContext: ");
    s.push_str(&session.context.to_string());
    s.push_str("\n\nHistory (most recent last):\n");
    for step in session.history.iter() {
        match step {
            Step::ToolCall(inv) => {
                s.push_str(&format!("  - tool_call {} {}\n", inv.tool, inv.args))
            }
            Step::ToolResult(r) => {
                let outcome = match &r.outcome {
                    Ok(v) => format!("ok {}", v),
                    Err(e) => format!("err {}", e),
                };
                s.push_str(&format!("  - tool_result {} {}\n", r.tool, outcome));
            }
            _ => {}
        }
    }
    match observation {
        crate::agent::types::Observation::Tick => {
            s.push_str("\nObservation: (first turn — call a tool)\n")
        }
        crate::agent::types::Observation::ToolResult { tool, value, error } => {
            s.push_str(&format!(
                "\nObservation: tool `{tool}` returned {} = {}\n",
                error.as_deref().unwrap_or("ok"),
                value
            ));
        }
        crate::agent::types::Observation::UserReply(text) => {
            s.push_str(&format!("\nObservation: user reply: {text}\n"));
        }
    }
    s
}

pub fn build_plan_prompt(tools: &[String]) -> String {
    let tool_list = if tools.is_empty() {
        "(no tools)".to_string()
    } else {
        tools.join(", ")
    };
    format!(
        "You are a planner inside the odyssey kernel. Tools: {tool_list}. \
         Produce a numbered plan (1., 2., 3., ...). Plain text only; \
         do not call any tools."
    )
}

/// Build the `tools` array for native tool calling. Each
/// entry is the tool's name, a description (from
/// `tool_schema.description` if present), and a JSON Schema
/// for the arguments. Tools without a `tool_schema` are
/// silently skipped — they remain reachable by name
/// through the cspace, just not advertised to the LLM.
pub fn collect_tool_schemas(cspace: &CapabilitySpace, allowed_tools: &[String]) -> Value {
    let tool_list: Vec<String> = if allowed_tools.is_empty() {
        cspace.enumerate().into_iter().map(|m| m.name).collect()
    } else {
        allowed_tools.to_vec()
    };
    let mut out: Vec<Value> = Vec::new();
    for name in tool_list {
        let Some(cap) = cspace.lookup_by_name(&name) else {
            continue;
        };
        let meta = cap.meta();
        let schema = match meta.tool_schema.as_ref() {
            Some(s) => s,
            None => continue,
        };
        let description = schema
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let parameters = schema
            .get("input_schema")
            .cloned()
            .unwrap_or(json!({ "type": "object" }));
        out.push(json!({
            "name": name,
            "description": description,
            "parameters": parameters,
        }));
    }
    Value::Array(out)
}

/// Extract the first `tool_call` from a native LLM response.
pub fn first_native_tool_call(llm_resp: &Value) -> Option<ToolInvocation> {
    let tc = llm_resp.get("tool_calls")?.as_array()?.first()?;
    let name = tc.get("name").and_then(Value::as_str)?.to_string();
    let arguments = tc.get("arguments").cloned().unwrap_or(Value::Null);
    Some(ToolInvocation {
        tool: name,
        args: arguments,
    })
}

/// Parse a text-mode LLM reply. Recognises the agent's
/// JSON protocol (`tool_call`/`final`) and falls back to
/// treating plain text as a final answer.
pub fn parse_llm_reply(
    text: &str,
    history: &mut History,
    allowed_tools: &[String],
    cspace: &CapabilitySpace,
) -> Result<(Step, SessionStatus), AgentError> {
    let trimmed = text.trim();
    if trimmed.starts_with('{')
        && let Ok(v) = serde_json::from_str::<Value>(trimmed)
    {
        if let Some(tc) = v.get("tool_call") {
            let tool = tc
                .get("tool")
                .and_then(Value::as_str)
                .ok_or_else(|| AgentError::LlmFailed("tool_call missing `tool`".into()))?
                .to_string();
            let args = tc.get("args").cloned().unwrap_or(Value::Null);
            if !allowed_tools.is_empty() && !allowed_tools.contains(&tool) {
                return Err(AgentError::ToolDenied {
                    tool,
                    reason: "not in allowed_tools",
                });
            }
            let cap = cspace
                .lookup_by_name(&tool)
                .ok_or_else(|| AgentError::ToolUnknown(tool.clone()))?;
            let outcome = cap
                .invoke_dyn_typed(Rights::INVOKE, args.clone())
                .map_err(|e| AgentError::ToolFailed {
                    tool: tool.clone(),
                    error: e.to_string(),
                });
            let tr = ToolResult {
                tool: tool.clone(),
                outcome: outcome.map_err(|e| e.to_string()),
            };
            let invocation = ToolInvocation {
                tool: tool.clone(),
                args,
            };
            history.push(Step::ToolCall(invocation));
            history.push(Step::ToolResult(tr.clone()));
            return Ok((Step::ToolResult(tr), SessionStatus::AwaitingObservation));
        }
        if let Some(final_v) = v.get("final") {
            let f = FinalAnswer {
                value: final_v.clone(),
                reason: FinalReason::Goal,
            };
            history.push(Step::Final(f.clone()));
            return Ok((Step::Final(f), SessionStatus::Done));
        }
    }
    let f = FinalAnswer {
        value: Value::String(text.to_string()),
        reason: FinalReason::Goal,
    };
    history.push(Step::Final(f.clone()));
    Ok((Step::Final(f), SessionStatus::Done))
}
