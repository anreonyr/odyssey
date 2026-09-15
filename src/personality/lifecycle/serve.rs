//! Personality / lifecycle / serve — HTTP bridge for capability dispatch.
//!
//! Phase 8: merged the Phase 5 split (`runtime/http_bridge.rs`
//! for the axum router + `runtime/lifecycle/shutdown.rs` for
//! the spawn wrapper) into one cohesive module.
//!
//! Routes:
//!
//! - `GET  /api/caps`    → list capabilities
//! - `POST /api/invoke`  → invoke a sync capability
//! - `POST /api/stream`  → open a streaming capability (SSE)

use std::convert::Infallible;
use std::future::Future;
use std::net::SocketAddr;

use axum::{
    extract::State,
    response::{
        sse::{Event, KeepAlive, Sse},
        Html, IntoResponse, Json,
    },
    routing::{get, post},
    Router,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;

use crate::capability::enforce::space::CapabilitySpace;
use crate::core::identity::kind::CapKind;
use crate::core::meta::chunk::CapabilityChunk;
use crate::core::meta::meta::CapabilityMeta;

#[derive(Clone)]
struct AppState {
    cspace: CapabilitySpace,
}

#[derive(Serialize)]
struct CapInfo {
    name: String,
    id: String,
    /// Serialised as the legacy `streaming: bool` field for
    /// HTTP API back-compat. The runtime meta now carries
    /// `kind: CapKind`; the bridge flattens it to a bool for
    /// the JSON payload. (A future `?as_kind=` extension can
    /// expose the full enum; today no client needs it.)
    streaming: bool,
    timeout_ms: u32,
    in_type: String,
    out_type: String,
}

#[derive(Deserialize)]
struct InvokeReq {
    capability: String,
    input: Value,
}

#[derive(Serialize)]
struct InvokeResp {
    capability: String,
    value: Value,
}

#[derive(Serialize)]
struct ErrorResp {
    error: String,
}

pub fn router(cspace: CapabilitySpace) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/caps", get(list_caps))
        .route("/api/invoke", post(invoke))
        .route("/api/stream", post(stream))
        .with_state(AppState { cspace })
}

async fn index() -> impl IntoResponse {
    // Phase 8: the HTML UI moves out of the binary's
    // `include_str!` — the example binary now reads it from
    // `examples/frontend/index.html` at runtime. The library
    // stays UI-free.
    Html(include_str!("../../../examples/frontend/index.html"))
}

async fn list_caps(State(state): State<AppState>) -> Json<Vec<CapInfo>> {
    Json(
        state
            .cspace
            .enumerate()
            .into_iter()
            .map(|m: CapabilityMeta| CapInfo {
                name: m.name.clone(),
                id: m.id.to_string(),
                streaming: m.kind == CapKind::Stream,
                timeout_ms: m.timeout_ms,
                in_type: m.in_type.clone(),
                out_type: m.out_type.clone(),
            })
            .collect(),
    )
}

async fn invoke(
    State(state): State<AppState>,
    Json(req): Json<InvokeReq>,
) -> Result<Json<InvokeResp>, Json<ErrorResp>> {
    let cap = state.cspace.lookup_by_name(&req.capability).ok_or_else(|| {
        Json(ErrorResp { error: format!("capability not found: {}", req.capability) })
    })?;

    if cap.is_streaming() {
        return Err(Json(ErrorResp {
            error: format!("{} is streaming; use /api/stream", req.capability),
        }));
    }

    // `invoke_dyn_typed` returns the typed `CapabilityError`
    // (Phase 5 M4 invariant). The HTTP bridge renders it as a
    // string for the client; clients that care about the typed
    // variant (e.g. for pattern matching) can opt in via a
    // future `?as_error_variant=` extension to the response
    // shape. Today every typed variant renders to a useful
    // string, so this is the more informative default.
    match cap.invoke_dyn_typed(req.input) {
        Ok(value) => Ok(Json(InvokeResp { capability: req.capability, value })),
        Err(e) => Err(Json(ErrorResp { error: e.to_string() })),
    }
}

async fn stream(
    State(state): State<AppState>,
    Json(req): Json<InvokeReq>,
) -> Sse<impl futures_core::Stream<Item = Result<Event, Infallible>>> {
    let cap = state.cspace.lookup_by_name(&req.capability);

    let (event_tx, event_rx) = mpsc::channel::<Result<Event, Infallible>>(16);

    match cap {
        Some(c) if c.is_streaming() => match c.open_dyn(req.input) {
            Ok(rx) => {
                tokio::spawn(async move {
                    let mut stream = tokio_stream::wrappers::ReceiverStream::new(rx);
                    while let Some(chunk) = stream.next().await {
                        let ev = match chunk {
                            CapabilityChunk::Item(v) => {
                                Ok(Event::default().event("chunk").data(v.to_string()))
                            }
                            CapabilityChunk::Done => {
                                Ok(Event::default().event("done").data(""))
                            }
                        };
                        if event_tx.send(ev).await.is_err() {
                            break;
                        }
                    }
                });
            }
            Err(e) => {
                let msg = e;
                tokio::spawn(async move {
                    let _ = event_tx
                        .send(Ok(Event::default().event("error").data(msg)))
                        .await;
                });
            }
        },
        Some(_) => {
            let msg = format!("{} is not streaming; use /api/invoke", req.capability);
            tokio::spawn(async move {
                let _ = event_tx
                    .send(Ok(Event::default().event("error").data(msg)))
                    .await;
            });
        }
        None => {
            let msg = format!("capability not found: {}", req.capability);
            tokio::spawn(async move {
                let _ = event_tx
                    .send(Ok(Event::default().event("error").data(msg)))
                    .await;
            });
        }
    }

    Sse::new(tokio_stream::wrappers::ReceiverStream::new(event_rx))
        .keep_alive(KeepAlive::default())
}

pub async fn serve(
    addr: SocketAddr,
    cspace: CapabilitySpace,
    shutdown: impl Future<Output = ()> + Send + 'static,
) {
    let app = router(cspace);
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    eprintln!("[http] listening on http://{addr}");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
        .expect("serve");
    eprintln!("[http] shut down");
}

/// Spawn the HTTP bridge on `addr` and return the task handle.
///
/// The bridge runs until either side returns from the serve
/// future. We pass a Ctrl-C future to `serve` so the orchestrator
/// can shut the bridge down by simply dropping the awaiter.
pub fn spawn_http_bridge(addr: SocketAddr, cspace: CapabilitySpace) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        serve(
            addr,
            cspace,
            async {
                let _ = tokio::signal::ctrl_c().await;
            },
        )
        .await;
    })
}
