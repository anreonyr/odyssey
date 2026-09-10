//! HTTP plugin — `HttpResource: Resource`. Sync only.
//!
//! Phase 4 P4.1 — provides the `http_request` capability that
//! Generator's `HttpModel` consumes (via the binding table).
//!
//! ## Why this is a plugin, not a stdlib HTTP client
//!
//! Generator must not "own" network permission. Per the
//! Phase 4 red line, all authority beyond what a capability
//! grants stays out of Generator. By exposing HTTP as a
//! capability, the **environment** decides whether Generator
//! gets network access at all — P4.5 proves this by giving
//! or withholding the `http` capability slot.
//!
//! ## Backend
//!
//! `HttpResource` is backed by an in-memory response table
//! (URL → canned body). This is a **mock** backend, suitable
//! for tests and offline proofs. A real backend (reqwest,
//! ureq, hyper) drops in behind the same `invoke()` shape
//! later; the only thing that changes is the response lookup.
//!
//! ## Input shape
//!
//! ```json
//! { "method": "POST",
//!   "url":    "/llm/v1/complete",
//!   "body":   {"prompt": "..."} }
//! ```
//!
//! Output:
//!
//! ```json
//! { "status": 200,
//!   "body":   <any JSON the table returned for this URL> }
//! ```

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use cordis::{plugin_with, Context, Injection, LogLevel, Plugin};
use serde_json::{json, Value};

use crate::kernel::{Resource, Slot};

/// URL → canned response body. Stored as raw JSON so callers
/// can shape the response exactly (string, object, list).
type ResponseTable = Arc<RwLock<HashMap<String, Value>>>;

pub struct HttpResource {
    responses: ResponseTable,
}

impl HttpResource {
    pub fn new() -> Self {
        Self { responses: Arc::new(RwLock::new(HashMap::new())) }
    }

    /// Seed the mock backend with a URL → response mapping.
    /// Replaces any existing entry for the same URL.
    pub fn set(&self, url: impl Into<String>, body: Value) {
        let mut tbl = self.responses.write().expect("http.responses lock");
        tbl.insert(url.into(), body);
    }

    /// Shared handle to the response table. Tests use this to
    /// seed canned responses without going through `set`.
    pub fn table(&self) -> ResponseTable {
        Arc::clone(&self.responses)
    }
}

impl Default for HttpResource {
    fn default() -> Self {
        Self::new()
    }
}

impl Resource for HttpResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let url = input
            .get("url")
            .and_then(Value::as_str)
            .ok_or_else(|| "http: missing 'url' field".to_string())?;
        let method = input
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("GET");
        // Mock backend only honours the URL; method is logged
        // but not enforced. Real backends will switch on method.
        let _ = method;
        let tbl = self
            .responses
            .read()
            .map_err(|e| format!("http: response table lock poisoned: {e}"))?;
        match tbl.get(url) {
            Some(body) => Ok(json!({ "status": 200, "body": body })),
            None => Ok(json!({
                "status": 404,
                "body":   { "error": format!("no mock response for {url}") }
            })),
        }
    }
}

/// Build a fresh `HttpResource`. Boot uses one; tests can
/// share a response table via [`HttpResource::table`].
pub fn handler() -> Arc<HttpResource> {
    Arc::new(HttpResource::new())
}

pub fn http_plugin() -> Arc<dyn Plugin> {
    plugin_with(
        "http",
        vec![Injection::from("slot:http")],
        |ctx: Context, _cfg: ()| async move {
            let slot: Arc<Slot<HttpResource>> = ctx.require("slot:http")?;
            ctx.logger().log(
                LogLevel::Info,
                format!(
                    "http plugin: slot={} cap_id={}",
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

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn invoke(resource: &HttpResource, json_body: &str) -> Result<Value, String> {
        resource.invoke(serde_json::from_str(json_body).unwrap())
    }

    #[test]
    fn invoke_seeded_url_returns_200_with_body() {
        let r = HttpResource::new();
        r.set("/llm", json!({"completion": "hi"}));
        let out = invoke(&r, r#"{"method":"POST","url":"/llm","body":{"prompt":"x"}}"#).unwrap();
        assert_eq!(out["status"], 200);
        assert_eq!(out["body"], json!({"completion": "hi"}));
    }

    #[test]
    fn invoke_unknown_url_returns_404() {
        let r = HttpResource::new();
        let out = invoke(&r, r#"{"method":"GET","url":"/absent"}"#).unwrap();
        assert_eq!(out["status"], 404);
        assert!(out["body"]["error"].as_str().unwrap().contains("/absent"));
    }

    #[test]
    fn invoke_missing_url_is_error() {
        let r = HttpResource::new();
        let err = invoke(&r, r#"{"method":"GET"}"#).unwrap_err();
        assert!(err.contains("missing 'url'"));
    }

    #[test]
    fn method_defaults_to_get() {
        let r = HttpResource::new();
        // method is unused in the mock, but should not cause an
        // error when omitted.
        let out = invoke(&r, r#"{"url":"/x"}"#).unwrap();
        assert_eq!(out["status"], 404);
    }

    #[test]
    fn set_overwrites_existing_url() {
        let r = HttpResource::new();
        r.set("/llm", json!("v1"));
        r.set("/llm", json!("v2"));
        let out = invoke(&r, r#"{"url":"/llm"}"#).unwrap();
        assert_eq!(out["body"], json!("v2"));
    }

    #[test]
    fn manifest_publishes_request_action() {
        let m = crate::plugins::http::manifest::manifest();
        let authority = &m.exposes[0].authority;
        assert_eq!(authority.operation_for("request").unwrap(), "HTTP_REQUEST");
        assert!(!m.exposes[0].streaming, "http must be sync");
    }
}
