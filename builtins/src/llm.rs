//! LLM builtin — a single plugin exposing two capabilities:
//!
//! - `llm_complete` (contract `"llm_complete"`) — text generation.
//! - `llm_embed`   (contract `"llm_embed"`)   — text → vector.
//!
//! Both share a backend via `Arc<dyn LlmBackend>`. The MVP ships a
//! `MockLlmBackend` that returns deterministic responses so the
//! agent's loop can be tested without a real provider. Swapping in
//! OpenAI / Anthropic / Ollama later means implementing
//! `LlmBackend` and registering the new plugin in `examples/basic.rs`.
//!
//! ## Two caps, one plugin
//!
//! Each cap has its own `CapabilityDecl::contract_name` so the
//! resolver can match `requires("llm_complete", "llm_complete")`
//! against it. The single `LlmBuiltin::mint` function dispatches
//! on `decl.name` to mint the right resource type — the same
//! pattern the agent uses for its 7 caps.
//!
//! ## Why one plugin, not two
//!
//! LLM complete and embed share the same provider config (model,
//! API key, base URL). Splitting them into two plugins would mean
//! duplicating that config or introducing a third plugin for the
//! shared provider. One plugin with two caps is the minimal
//! decomposition that keeps provider state shared.

use std::sync::Arc;

use odyssey::capability::enforce::quota::CapabilityBudget;
use odyssey::core::Resource;
use odyssey::core::contract::builtin::BuiltinManifest;
use odyssey::core::identity::ids::{PluginId, SlotId};
use odyssey::core::identity::kind::CapKind;
use odyssey::core::manifest::manifest::{CapabilityDecl, ManifestBuilder, PluginManifest};
use odyssey::personality::composition::resolve::ResolvedBinding;
use odyssey::personality::lifecycle::mint::CapabilityFactory;
use odyssey::personality::lifecycle::run::{MintFn, RuinFn, default_ruin};
use serde_json::{Value, json};

pub const CONTRACT_COMPLETE: &str = "llm_complete";
pub const CONTRACT_EMBED: &str = "llm_embed";
pub const NAME_COMPLETE: &str = "llm_complete";
pub const NAME_EMBED: &str = "llm_embed";

// ---------------------------------------------------------------------------
// Backend trait — provider abstraction
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct CompleteRequest {
    pub prompt: String,
    pub system: Option<String>,
    pub stop: Vec<String>,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
    /// Native tool definitions. When non-empty, the backend
    /// should pass them to the LLM as a `tools` array (OpenAI
    /// and most providers support this); the LLM's reply may
    /// then include structured `tool_calls` instead of plain
    /// text. Backends that don't support native tool calling
    /// (e.g. the mock) ignore this field — the agent's
    /// text-based protocol still works.
    pub tools: Vec<ToolDefinition>,
}

/// One tool the LLM may call. The shape is OpenAI-compatible:
/// `{"type":"function","function":{"name":...,"description":...,
/// "parameters":<JSON Schema>}}`. The `name` and
/// `description` show up in the system prompt and the LLM's
/// `tool_calls` response; `parameters` is the JSON Schema
/// the LLM uses to fill in `arguments`.
#[derive(Debug, Clone)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    /// JSON Schema for the tool's arguments. The LLM will
    /// return arguments that match this schema. Stored as
    /// raw `serde_json::Value` so any schema shape is
    /// supported.
    pub parameters: Value,
}

#[derive(Debug, Clone)]
pub struct CompleteResponse {
    /// The assistant's text reply. Empty if the LLM only
    /// emitted `tool_calls` (OpenAI does this on purpose —
    /// it forces the caller to feed the tool result back
    /// before the LLM writes prose).
    pub text: String,
    /// Native tool calls the LLM wants to make. Empty for
    /// the text-only path. Each call carries the tool's
    /// local name, a provider-issued id, and a JSON object
    /// matching the tool's `parameters` schema.
    pub tool_calls: Vec<ToolCall>,
    pub finish_reason: String,
    pub usage: Usage,
}

/// One tool call in an LLM response. The `id` is
/// provider-issued; the agent doesn't interpret it, but
/// threads it through so a follow-up call can address the
/// specific call (some providers' chat APIs require this).
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// JSON arguments the LLM produced. Already validated by
    /// the LLM against the tool's `parameters` schema, but
    /// the kernel does not enforce the schema at the type
    /// level — `ToolDescriptor` is the audit/inspection path.
    pub arguments: Value,
}

#[derive(Debug, Clone, Default)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

/// One event from a streaming chat-completions call.
///
/// Real OpenAI providers (and any OpenAI-compatible surface
/// that supports `stream: true`) emit a sequence of `delta`
/// chunks, each carrying a piece of the assistant's text
/// and (for tool calls) an incremental `tool_calls` entry.
/// The `Done` event carries the final assembled response
/// with usage stats — the rest of the system reads only
/// this. `Error` is for failures that happen mid-stream.
///
/// Backends that don't natively stream can ignore the
/// `Delta` events and emit a single `Done` carrying the
/// result of an internal `complete` call.
pub enum StreamEvent {
    /// A piece of the assistant's text. Multiple `Delta`s
    /// concatenate to form the full text. May be a partial
    /// UTF-8 sequence at chunk boundaries; the consumer
    /// should buffer if it needs valid UTF-8 at every
    /// step.
    Delta(String),
    /// Terminal event. The receiver should treat the
    /// carried `CompleteResponse` as the final response
    /// (text, tool_calls, usage, finish_reason).
    Done(CompleteResponse),
    /// Mid-stream failure. The receiver should treat the
    /// stream as aborted; the error message is a free-form
    /// `String` for diagnostics.
    Error(String),
}

pub trait LlmBackend: Send + Sync {
    /// Single-shot completion. Used by tools and tests that
    /// don't care about per-token latency. The default
    /// `complete_stream` impl wraps this in a one-event
    /// stream, so backends that only implement `complete`
    /// are streaming-ready by inheritance.
    fn complete(&self, req: CompleteRequest) -> Result<CompleteResponse, String>;
    /// Stream a completion. The backend writes events to
    /// `tx` and closes the channel when done. The
    /// `Done` event carries the final assembled response.
    /// Errors before the stream starts return via
    /// `Result::Err`; mid-stream errors arrive as
    /// `StreamEvent::Error`.
    ///
    /// The default impl is a one-event stream that just
    /// forwards the result of `complete`; backends that
    /// don't natively stream need not override this.
    fn complete_stream(
        &self,
        req: CompleteRequest,
        tx: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<(), String> {
        let resp = self.complete(req)?;
        // The `tx.send` error means the consumer dropped the
        // receiver mid-call; we surface that as Ok(()) —
        // the backend is done, the consumer doesn't care.
        let _ = tx.blocking_send(StreamEvent::Done(resp));
        Ok(())
    }
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String>;
}

// ---------------------------------------------------------------------------
// Mock backend — deterministic, no network, for tests
// ---------------------------------------------------------------------------

/// Mock LLM backend. Returns canned responses that follow the
/// agent's protocol:
///
/// - The system prompt the agent builds includes a section
///   `__TOOL_CALL_INSTRUCTION__` that asks the LLM to emit a JSON
///   object when it wants to call a tool. The mock detects this
///   request and returns a tool_call JSON.
/// - A second call (after observing the tool result) is asked to
///   produce a final answer. The mock returns `__FINAL__` text
///   that the agent recognises.
///
/// The agent parses the LLM's text output. If the text starts
/// with `{` and contains `"tool_call"`, it's a tool call. If it
/// starts with `{` and contains `"final"`, it's a final answer.
pub struct MockLlmBackend;

impl LlmBackend for MockLlmBackend {
    fn complete(&self, req: CompleteRequest) -> Result<CompleteResponse, String> {
        // The agent's protocol lives in the *system* prompt,
        // not the user prompt. The mock looks for the
        // directives in `req.system` and dispatches the
        // appropriate canned response. Real providers would
        // parse the agent's structured instructions
        // themselves; the mock just keeps the test
        // deterministic.
        let system = req.system.as_deref().unwrap_or("");

        // If the agent passed `tools`, the mock pretends the
        // LLM chose the first one. This exercises the same
        // native-tool-call path the OpenAI backend takes,
        // so the agent's tool-execution logic is covered
        // even when the mock is the backend. We return
        // both `text` and `tool_calls` (the shape real
        // OpenAI providers emit when the model explains
        // its reasoning and then calls a tool) so the
        // agent's prose-preservation path is also
        // exercised end-to-end.
        if !req.tools.is_empty() && system.contains("__AGENT_FIRST_ACTION__") {
            let tool = &req.tools[0];
            return Ok(CompleteResponse {
                text: "I'll call the first tool with the canned value.".into(),
                tool_calls: vec![ToolCall {
                    id: "call_mock_1".into(),
                    name: tool.name.clone(),
                    arguments: json!({ "input": "hello from mock llm" }),
                }],
                finish_reason: "tool_calls".into(),
                usage: Usage {
                    prompt_tokens: req.prompt.len() as u32,
                    completion_tokens: 12,
                },
            });
        }

        if system.contains("__AGENT_FIRST_ACTION__") {
            return Ok(CompleteResponse {
                text: json!({
                    "tool_call": {
                        "tool": "echo",
                        "args": { "input": "hello from mock llm" }
                    }
                })
                .to_string(),
                tool_calls: vec![],
                finish_reason: "stop".into(),
                usage: Usage {
                    prompt_tokens: req.prompt.len() as u32,
                    completion_tokens: 16,
                },
            });
        }

        if system.contains("__AGENT_FINAL_AFTER_TOOL__") {
            return Ok(CompleteResponse {
                text: json!({
                    "final": {
                        "echoed": "hello from mock llm",
                        "note": "the mock finished after one tool call"
                    }
                })
                .to_string(),
                tool_calls: vec![],
                finish_reason: "stop".into(),
                usage: Usage {
                    prompt_tokens: req.prompt.len() as u32,
                    completion_tokens: 12,
                },
            });
        }

        // Default: plain text echo. The agent treats any non-JSON
        // text as a final answer.
        Ok(CompleteResponse {
            text: format!("mock llm ack: {}", req.prompt),
            tool_calls: vec![],
            finish_reason: "stop".into(),
            usage: Usage {
                prompt_tokens: req.prompt.len() as u32,
                completion_tokens: (req.prompt.len() as u32) / 4,
            },
        })
    }

    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        // Deterministic 4-dimensional embedding: each text's
        // first 4 chars' ASCII values, normalised. Real backends
        // would return 768/1024/1536-dim vectors; the test
        // doesn't care about the exact shape, only that the
        // memory backend receives something consistent.
        Ok(texts
            .iter()
            .map(|t| {
                let bytes = t.as_bytes();
                let mut v = [0.0f32; 4];
                for (i, b) in bytes.iter().take(4).enumerate() {
                    v[i] = *b as f32 / 255.0;
                }
                let norm = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2] + v[3] * v[3]).sqrt();
                if norm > 0.0 {
                    for x in v.iter_mut() {
                        *x /= norm;
                    }
                }
                v.to_vec()
            })
            .collect())
    }
}

// ---------------------------------------------------------------------------
// OpenAI-compatible backend — real HTTP provider
// ---------------------------------------------------------------------------

/// Default model for chat completions.
pub const DEFAULT_OPENAI_CHAT_MODEL: &str = "gpt-4o-mini";
/// Default model for embeddings.
pub const DEFAULT_OPENAI_EMBED_MODEL: &str = "text-embedding-3-small";

/// OpenAI-compatible backend. Speaks the standard
/// `POST {base}/chat/completions` and `POST {base}/embeddings`
/// surface; any provider that conforms (OpenAI, keyless
/// proxies, llama.cpp, vLLM, …) plugs in here.
///
/// ## Why shell out to `curl`
///
/// The kernel is intended to be lightweight and to keep its
/// dependency surface tight. Adding `reqwest` (or `ureq` +
/// `rustls`) for one LLM provider pulls a chain of TLS
/// crates into the workspace. The runtime here is a sync
/// `Resource::invoke` boundary, and the kernel already has
/// `tokio` for the orchestrator — but the LLM plugin doesn't
/// need an embedded TLS stack. We shell out to the system
/// `curl` (every Linux/macOS dev box has it), parse the
/// response body, and return.
///
/// If a future caller needs zero system dependencies, swap
/// this for an embedded client. The `LlmBackend` trait stays
/// the same; only this impl changes.
pub struct OpenAiBackend {
    base_url: String,
    api_key: String,
    chat_model: String,
    embed_model: String,
    timeout_secs: u64,
}

impl OpenAiBackend {
    /// Build from environment. Recognised variables:
    ///
    /// - `OPENAI_API_BASE` (required to enable this backend) —
    ///   e.g. `https://api.openai.com/v1` or
    ///   `https://keylessai.thryx.workers.dev/v1`. The
    ///   trailing `/v1` is part of the base.
    /// - `OPENAI_API_KEY` (optional; some proxies accept
    ///   `"not-needed"` or empty). Sent as
    ///   `Authorization: Bearer <key>` when non-empty.
    /// - `ODYSSEY_LLM_CHAT_MODEL` (default
    ///   `DEFAULT_OPENAI_CHAT_MODEL`).
    /// - `ODYSSEY_LLM_EMBED_MODEL` (default
    ///   `DEFAULT_OPENAI_EMBED_MODEL`).
    /// - `ODYSSEY_LLM_TIMEOUT_SECS` (default 60).
    pub fn from_env() -> Result<Self, String> {
        let base_url = std::env::var("OPENAI_API_BASE")
            .map_err(|_| "OPENAI_API_BASE is not set".to_string())?;
        let api_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
        let chat_model = std::env::var("ODYSSEY_LLM_CHAT_MODEL")
            .unwrap_or_else(|_| DEFAULT_OPENAI_CHAT_MODEL.to_string());
        let embed_model = std::env::var("ODYSSEY_LLM_EMBED_MODEL")
            .unwrap_or_else(|_| DEFAULT_OPENAI_EMBED_MODEL.to_string());
        let timeout_secs = std::env::var("ODYSSEY_LLM_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(60);
        Ok(Self {
            base_url,
            api_key,
            chat_model,
            embed_model,
            timeout_secs,
        })
    }

    /// Build with explicit config — used by tests that don't
    /// want to touch the environment.
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        chat_model: impl Into<String>,
        embed_model: impl Into<String>,
    ) -> Result<Self, String> {
        Ok(Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            chat_model: chat_model.into(),
            embed_model: embed_model.into(),
            timeout_secs: 60,
        })
    }

    /// POST `body` to `endpoint` (relative to `base_url`) and
    /// return the response body as a UTF-8 string. `endpoint`
    /// is the path component (e.g. `"chat/completions"`).
    fn http_post(&self, endpoint: &str, body: &Value) -> Result<String, String> {
        let url = format!("{}/{}", self.base_url.trim_end_matches('/'), endpoint);
        let body_str = serde_json::to_string(body)
            .map_err(|e| format!("openai: serialise body: {e}"))?;

        let mut cmd = std::process::Command::new("curl");
        cmd.arg("--silent")                // no progress meter
            .arg("--show-error")            // surface errors on stderr
            .arg("--max-time").arg(self.timeout_secs.to_string())
            .arg("--write-out").arg("\n__HTTP_STATUS__:%{http_code}\n")
            .arg("--header").arg("Content-Type: application/json");
        if !self.api_key.is_empty() {
            cmd.arg("--header")
                .arg(format!("Authorization: Bearer {}", self.api_key));
        }
        cmd.arg("--data").arg(&body_str).arg(&url);

        let output = cmd
            .output()
            .map_err(|e| format!("openai: spawn curl: {e}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // curl exits with code 22 for HTTP 4xx/5xx when
            // --fail is used. We don't use --fail, so a
            // non-zero exit is a transport error.
            return Err(format!(
                "openai: curl exit {:?}: {}",
                output.status.code(),
                stderr.chars().take(400).collect::<String>()
            ));
        }
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        // Body is everything before the marker; status is the
        // number after.
        let (body_part, status_part) = match stdout.rsplit_once("__HTTP_STATUS__:") {
            Some((b, s)) => (b.trim().to_string(), s.trim().to_string()),
            None => {
                return Err(format!(
                    "openai: response missing status marker; body head: {}",
                    stdout.chars().take(200).collect::<String>()
                ))
            }
        };
        let status: u32 = status_part
            .trim()
            .parse()
            .map_err(|e| format!("openai: parse status: {e}"))?;
        if !(200..300).contains(&status) {
            return Err(format!(
                "openai: HTTP {status}: {}",
                body_part.chars().take(400).collect::<String>()
            ));
        }
        Ok(body_part)
    }
}

impl OpenAiBackend {
    /// Build the chat-completions request body. Shared by
    /// `complete` and `complete_stream` so the two paths
    /// stay in lock-step on the wire shape. `stream` is
    /// not set here — `complete_stream` adds it after.
    fn chat_body(&self, req: &CompleteRequest) -> Value {
        let mut messages = Vec::new();
        if let Some(sys) = &req.system {
            messages.push(json!({ "role": "system", "content": sys }));
        }
        messages.push(json!({ "role": "user", "content": req.prompt }));

        let mut body = json!({
            "model": self.chat_model,
            "messages": messages,
            "temperature": req.temperature,
        });
        if !req.stop.is_empty() {
            body["stop"] = json!(req.stop);
        }
        if let Some(n) = req.max_tokens {
            body["max_tokens"] = json!(n);
        }
        // Native tool calling: when the agent passes tool
        // definitions, serialise them in OpenAI's expected
        // shape and ask the LLM to choose one with
        // `tool_choice: "auto"`. Without this, the LLM only
        // sees tool names in the system prompt and we rely on
        // it following a JSON-in-text instruction, which
        // real models do inconsistently.
        if !req.tools.is_empty() {
            let tool_defs: Vec<Value> = req
                .tools
                .iter()
                .map(|t| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.parameters,
                        }
                    })
                })
                .collect();
            body["tools"] = json!(tool_defs);
            body["tool_choice"] = json!("auto");
        }
        body
    }

    /// Parse a single OpenAI chat-completions response
    /// (sync or the final assembled streaming response) into
    /// the kernel's `CompleteResponse`. Shared by the two
    /// paths so the answer shape is identical.
    fn parse_chat_response(v: &Value) -> Result<CompleteResponse, String> {
        let choice = v
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .ok_or_else(|| "openai: empty `choices`".to_string())?;
        let message = choice
            .get("message")
            .ok_or_else(|| "openai: missing `choices[0].message`".to_string())?;

        let text = message
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        let tool_calls: Vec<ToolCall> = message
            .get("tool_calls")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .map(|tc| {
                        let id = tc
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        let name = tc
                            .get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        let arguments = tc
                            .get("function")
                            .and_then(|f| f.get("arguments"))
                            .map(|args| {
                                if let Some(s) = args.as_str() {
                                    serde_json::from_str::<Value>(s)
                                        .unwrap_or_else(|_| Value::String(s.to_string()))
                                } else {
                                    args.clone()
                                }
                            })
                            .unwrap_or(Value::Null);
                        ToolCall { id, name, arguments }
                    })
                    .collect()
            })
            .unwrap_or_default();

        let finish_reason = choice
            .get("finish_reason")
            .and_then(Value::as_str)
            .unwrap_or(if !tool_calls.is_empty() { "tool_calls" } else { "stop" })
            .to_string();
        let usage = v.get("usage").cloned().unwrap_or(Value::Null);
        let prompt_tokens = usage
            .get("prompt_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        let completion_tokens = usage
            .get("completion_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;

        Ok(CompleteResponse {
            text,
            tool_calls,
            finish_reason,
            usage: Usage {
                prompt_tokens,
                completion_tokens,
            },
        })
    }
}

impl LlmBackend for OpenAiBackend {
    fn complete(&self, req: CompleteRequest) -> Result<CompleteResponse, String> {
        let body = self.chat_body(&req);
        let text = self.http_post("chat/completions", &body)?;
        let v: Value = serde_json::from_str(&text)
            .map_err(|e| format!("openai: parse JSON: {e}; body head: {}",
                text.chars().take(200).collect::<String>()))?;
        Self::parse_chat_response(&v)
    }

    fn complete_stream(
        &self,
        req: CompleteRequest,
        tx: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<(), String> {
        // Build the chat body, set `stream: true`, serialise.
        let mut body = self.chat_body(&req);
        body["stream"] = json!(true);
        let body_str = serde_json::to_string(&body)
            .map_err(|e| format!("openai stream: serialise body: {e}"))?;
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let api_key = self.api_key.clone();
        let timeout_secs = self.timeout_secs;

        // Spawn a thread that runs curl --no-buffer, parses
        // SSE chunks, and writes `StreamEvent`s to `tx`.
        // Spawning is necessary because the agent calls
        // `complete_stream` from a sync `Resource::invoke`
        // boundary; the agent drains `tx` from a second
        // thread.
        let tx_thread = tx.clone();
        let _handle = std::thread::spawn(move || {
            let mut cmd = std::process::Command::new("curl");
            cmd.arg("--silent")
                .arg("--show-error")
                .arg("--no-buffer") // flush SSE chunks as they arrive
                .arg("--max-time").arg(timeout_secs.to_string())
                .arg("--write-out").arg("\n__HTTP_STATUS__:%{http_code}\n")
                .arg("--header").arg("Content-Type: application/json");
            if !api_key.is_empty() {
                cmd.arg("--header")
                    .arg(format!("Authorization: Bearer {}", api_key));
            }
            cmd.arg("--data").arg(&body_str).arg(&url);

            let output = match cmd.output() {
                Ok(o) => o,
                Err(e) => {
                    let _ = tx_thread.blocking_send(StreamEvent::Error(
                        format!("openai stream: spawn curl: {e}"),
                    ));
                    return;
                }
            };
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let _ = tx_thread.blocking_send(StreamEvent::Error(format!(
                    "openai stream: curl exit {:?}: {}",
                    output.status.code(),
                    stderr.chars().take(400).collect::<String>()
                )));
                return;
            }
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();

            // Split off the trailing status marker (curl's
            // --write-out). Anything before the marker is
            // SSE: lines like
            //   data: {"choices":[...]}
            // separated by blank lines, with `data: [DONE]`
            // as the terminal line.
            let (body, status_part) = match stdout.rsplit_once("__HTTP_STATUS__:") {
                Some((b, s)) => (b, s.trim()),
                None => {
                    let _ = tx_thread.blocking_send(StreamEvent::Error(
                        "openai stream: missing status marker".to_string(),
                    ));
                    return;
                }
            };
            let status: u32 = match status_part.parse() {
                Ok(n) => n,
                Err(e) => {
                    let _ = tx_thread.blocking_send(StreamEvent::Error(format!(
                        "openai stream: parse status: {e}"
                    )));
                    return;
                }
            };
            if !(200..300).contains(&status) {
                let head: String = body.chars().take(400).collect();
                let _ = tx_thread.blocking_send(StreamEvent::Error(format!(
                    "openai stream: HTTP {status}: {head}"
                )));
                return;
            }

            // Walk the SSE line stream. Each event is a
            // `data: <json>` line; the empty line separates
            // events. We accumulate `delta.content` into a
            // `text` buffer and emit a `Delta` for each
            // non-empty piece. The `delta.tool_calls` array
            // is accumulated too (each chunk adds fields
            // incrementally) — for the MVP we just take the
            // final, fully-formed tool_calls from the last
            // chunk. A future pass would merge incremental
            // deltas properly.
            let mut text_buf = String::new();
            let mut tool_calls_buf: Vec<ToolCall> = Vec::new();
            let mut finish_reason = String::new();
            let mut prompt_tokens: u32 = 0;
            let mut completion_tokens: u32 = 0;
            let mut id_buf = String::new();
            let mut model_buf = String::new();

            for line in body.lines() {
                let line = line.trim_end_matches('\r');
                let Some(rest) = line.strip_prefix("data: ") else {
                    continue;
                };
                if rest == "[DONE]" {
                    break;
                }
                let v: Value = match serde_json::from_str(rest) {
                    Ok(v) => v,
                    Err(_) => continue, // skip malformed lines
                };
                if id_buf.is_empty() {
                    if let Some(id) = v.get("id").and_then(Value::as_str) {
                        id_buf = id.to_string();
                    }
                }
                if model_buf.is_empty() {
                    if let Some(m) = v.get("model").and_then(Value::as_str) {
                        model_buf = m.to_string();
                    }
                }
                if let Some(usage) = v.get("usage") {
                    prompt_tokens = usage
                        .get("prompt_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as u32;
                    completion_tokens = usage
                        .get("completion_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as u32;
                }
                let Some(choice) = v
                    .get("choices")
                    .and_then(Value::as_array)
                    .and_then(|a| a.first())
                else {
                    continue;
                };
                if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                    if !reason.is_empty() {
                        finish_reason = reason.to_string();
                    }
                }
                if let Some(delta) = choice.get("delta") {
                    if let Some(s) = delta.get("content").and_then(Value::as_str) {
                        if !s.is_empty() {
                            text_buf.push_str(s);
                            let _ = tx_thread.blocking_send(StreamEvent::Delta(s.to_string()));
                        }
                    }
                    if let Some(arr) = delta.get("tool_calls").and_then(Value::as_array) {
                        // For MVP: take the latest version of
                        // each tool call by `index`. The full
                        // merge logic (incremental `id`/`name`
                        // /`arguments` deltas) would be more
                        // accurate for very long tool calls.
                        for tc in arr {
                            let idx = tc
                                .get("index")
                                .and_then(Value::as_u64)
                                .unwrap_or(0) as usize;
                            while tool_calls_buf.len() <= idx {
                                tool_calls_buf.push(ToolCall {
                                    id: String::new(),
                                    name: String::new(),
                                    arguments: Value::Null,
                                });
                            }
                            if let Some(id) = tc.get("id").and_then(Value::as_str) {
                                if !id.is_empty() {
                                    tool_calls_buf[idx].id = id.to_string();
                                }
                            }
                            if let Some(func) = tc.get("function") {
                                if let Some(name) =
                                    func.get("name").and_then(Value::as_str)
                                {
                                    if !name.is_empty() {
                                        tool_calls_buf[idx].name = name.to_string();
                                    }
                                }
                                if let Some(args) = func.get("arguments") {
                                    let parsed = if let Some(s) = args.as_str() {
                                        serde_json::from_str::<Value>(s)
                                            .unwrap_or_else(|_| Value::String(s.to_string()))
                                    } else {
                                        args.clone()
                                    };
                                    tool_calls_buf[idx].arguments = parsed;
                                }
                            }
                        }
                    }
                }
            }

            if finish_reason.is_empty() {
                finish_reason = if !tool_calls_buf.is_empty() {
                    "tool_calls".into()
                } else {
                    "stop".into()
                };
            }

            let _ = tx_thread.blocking_send(StreamEvent::Done(CompleteResponse {
                text: text_buf,
                tool_calls: tool_calls_buf,
                finish_reason,
                usage: Usage {
                    prompt_tokens,
                    completion_tokens,
                },
            }));
            // Touching id_buf / model_buf silences unused-
            // warning if the future adds metadata passing.
            let _ = (id_buf, model_buf);
        });

        Ok(())
    }

    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        let body = json!({
            "model": self.embed_model,
            "input": texts,
        });
        let text = self.http_post("embeddings", &body)?;
        let v: Value = serde_json::from_str(&text)
            .map_err(|e| format!("openai embed: parse JSON: {e}"))?;
        let data = v
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| "openai embed: missing `data` array".to_string())?;
        let mut out = Vec::with_capacity(data.len());
        for entry in data {
            let vec = entry
                .get("embedding")
                .and_then(Value::as_array)
                .ok_or_else(|| "openai embed: entry missing `embedding`".to_string())?
                .iter()
                .filter_map(Value::as_f64)
                .map(|n| n as f32)
                .collect::<Vec<f32>>();
            out.push(vec);
        }
        if out.len() != texts.len() {
            return Err(format!(
                "openai embed: expected {} vectors, got {}",
                texts.len(),
                out.len()
            ));
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// Backend selection — env-driven
// ---------------------------------------------------------------------------

/// Pick the right backend for the environment. If
/// `OPENAI_API_BASE` is set, use the real provider; otherwise
/// the deterministic mock. Errors only if the env points at
/// a real provider but the configuration is broken (e.g.
/// the reqwest client fails to build).
pub fn backend_from_env() -> Result<Arc<dyn LlmBackend>, String> {
    if std::env::var("OPENAI_API_BASE").is_ok() {
        Ok(Arc::new(OpenAiBackend::from_env()?))
    } else {
        Ok(Arc::new(MockLlmBackend))
    }
}

// ---------------------------------------------------------------------------
// Resources — one per cap
// ---------------------------------------------------------------------------

pub struct LlmCompleteResource {
    pub backend: Arc<dyn LlmBackend>,
}

impl Resource for LlmCompleteResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let prompt = input
            .get("prompt")
            .and_then(Value::as_str)
            .ok_or_else(|| "llm_complete: expected `prompt` string".to_string())?;
        let system = input
            .get("system")
            .and_then(Value::as_str)
            .map(|s| s.to_string());
        let stop = input
            .get("stop")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();
        let temperature = input
            .get("temperature")
            .and_then(Value::as_f64)
            .map(|t| t as f32)
            .unwrap_or(0.7);
        let max_tokens = input
            .get("max_tokens")
            .and_then(Value::as_u64)
            .map(|n| n as u32);
        // `tools` is an array of `{name, description, parameters}`
        // objects. The agent passes these when it wants the
        // LLM to use native tool calling; the backend
        // serialises them into OpenAI's `tools` field.
        let tools = input
            .get("tools")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .map(|t| {
                        let name = t
                            .get("name")
                            .and_then(Value::as_str)
                            .ok_or_else(|| {
                                "llm_complete: tool entry missing `name`".to_string()
                            })?
                            .to_string();
                        let description = t
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        let parameters = t
                            .get("parameters")
                            .cloned()
                            .unwrap_or(json!({ "type": "object" }));
                        Ok(ToolDefinition { name, description, parameters })
                    })
                    .collect::<Result<Vec<ToolDefinition>, String>>()
            })
            .transpose()?
            .unwrap_or_default();

        let req = CompleteRequest {
            prompt: prompt.to_string(),
            system,
            stop,
            temperature,
            max_tokens,
            tools,
        };

        // Use the streaming path. The OpenAI backend spawns
        // a thread that runs `curl --no-buffer` and writes
        // `Delta`/`Done`/`Error` events to `tx`; the mock
        // uses the default `complete_stream` impl that
        // forwards `complete`'s result. We block-drain
        // `rx` for the final `Done` event. Per-token
        // `Delta`s are currently discarded by this caller —
        // they're a side-channel for `agent_stream`
        // subscribers; the resource-level return is
        // identical to the pre-streaming `complete` path.
        //
        // The agent's `advance` puts its `session_id` in
        // the input so we can route per-token `Delta`s to
        // that session's broadcast sender, which
        // `agent_stream` subscribers consume. The lookup is
        // a global; missing session is a no-op (the call
        // raced with a `cancel`, or the test didn't bother
        // to register one).
        let session_id = input
            .get("session_id")
            .and_then(Value::as_str)
            .map(String::from);
        let (tx, mut rx) = tokio::sync::mpsc::channel::<StreamEvent>(64);
        self.backend
            .complete_stream(req, tx)
            .map_err(|e| e)?;
        let resp = loop {
            match rx.blocking_recv() {
                Some(StreamEvent::Done(r)) => break r,
                Some(StreamEvent::Error(e)) => return Err(e),
                Some(StreamEvent::Delta(s)) => {
                    if let Some(sid) = session_id.as_deref() {
                        crate::agent_runtime::push_event_to_session(
                            sid,
                            crate::agent_runtime::AgentEvent::LlmDelta(s),
                        );
                    }
                    continue;
                }
                None => return Err("openai stream: closed before Done".to_string()),
            }
        };
        // The response carries both the assistant's text and
        // any tool calls the LLM made. Empty `tool_calls` is
        // represented as an empty array so callers can do
        // `if (resp.tool_calls.length) ...` without checking
        // for null.
        let tool_calls: Vec<Value> = resp
            .tool_calls
            .iter()
            .map(|tc| {
                json!({
                    "id": tc.id,
                    "name": tc.name,
                    "arguments": tc.arguments,
                })
            })
            .collect();
        Ok(json!({
            "text": resp.text,
            "tool_calls": tool_calls,
            "finish_reason": resp.finish_reason,
            "usage": {
                "prompt_tokens": resp.usage.prompt_tokens,
                "completion_tokens": resp.usage.completion_tokens,
            }
        }))
    }
}

pub struct LlmEmbedResource {
    backend: Arc<dyn LlmBackend>,
}

impl Resource for LlmEmbedResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let texts: Vec<String> = input
            .get("texts")
            .and_then(Value::as_array)
            .ok_or_else(|| "llm_embed: expected `texts` array".to_string())?
            .iter()
            .filter_map(Value::as_str)
            .map(|s| s.to_string())
            .collect();

        if texts.is_empty() {
            return Err("llm_embed: `texts` must contain at least one string".to_string());
        }

        let vectors = self.backend.embed(&texts).map_err(|e| e)?;
        Ok(json!({ "vectors": vectors }))
    }
}

// ---------------------------------------------------------------------------
// Builtin — one plugin, two caps
// ---------------------------------------------------------------------------

pub struct LlmBuiltin;

impl BuiltinManifest for LlmBuiltin {
    fn manifest(&self) -> PluginManifest {
        ManifestBuilder::new("llm")
            .expose(NAME_COMPLETE, CONTRACT_COMPLETE)
            .expose(NAME_EMBED, CONTRACT_EMBED)
            .host("dispatcher")
            .timeout_ms(30000)
            .build()
    }
}

impl LlmBuiltin {
    pub fn mint(
        &self,
        factory: &CapabilityFactory,
        plugin: &PluginId,
        decl: &CapabilityDecl,
        kind: CapKind,
        budget: CapabilityBudget,
        _bindings: &[ResolvedBinding],
    ) -> SlotId {
        // Backend selection: if `OPENAI_API_BASE` is set, use
        // the real HTTP provider; otherwise the deterministic
        // mock. Either way the two caps share the same backend
        // Arc, so a single provider instance serves both
        // `llm_complete` and `llm_embed` requests.
        //
        // The mint_fn is sync and returns SlotId; backend init
        // failures panic rather than returning Result, because
        // mint-time errors are configuration errors the
        // operator should see at boot, not as silent runtime
        // failures inside every invoke.
        let backend: Arc<dyn LlmBackend> = match backend_from_env() {
            Ok(b) => b,
            Err(e) => panic!("llm: backend init: {e}"),
        };

        // `factory.mint` is generic over `R: Resource`, so we
        // dispatch on the cap name and call the concrete
        // instantiation. Each arm produces a different `R` type,
        // but each is concrete — there's no `Arc<dyn Resource>`
        // path through the factory.
        match decl.name.as_str() {
            NAME_COMPLETE => factory.mint(
                kind,
                decl,
                plugin,
                budget,
                Arc::new(LlmCompleteResource {
                    backend: backend.clone(),
                }),
            ),
            NAME_EMBED => factory.mint(
                kind,
                decl,
                plugin,
                budget,
                Arc::new(LlmEmbedResource { backend }),
            ),
            other => panic!("llm: unexpected capability name `{other}`"),
        }
    }

    pub fn register() -> (PluginManifest, MintFn, RuinFn) {
        (
            LlmBuiltin.manifest(),
            |factory, plugin, decl, kind, budget, bindings| {
                LlmBuiltin.mint(factory, plugin, decl, kind, budget, bindings)
            },
            default_ruin,
        )
    }
}
