//! HTTP bridge — enumerates registered capabilities from the
//! `CapabilitySpace` and routes invocations through them.
//!
//!   GET  /api/caps    →  enumerate capabilities
//!   POST /api/invoke  →  invoke a sync capability
//!   POST /api/stream  →  open a streaming capability (SSE)

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
use crate::capability::{CapabilityChunk, CapabilitySpace};
use cordis::Context;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;

#[derive(Clone)]
struct AppState {
    cspace: CapabilitySpace,
}

#[derive(Serialize)]
struct CapInfo {
    name: String,
    id: String,
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

pub fn router(ctx: Context, cspace: CapabilitySpace) -> Router {
    let _ = ctx;
    Router::new()
        .route("/", get(index))
        .route("/api/caps", get(list_caps))
        .route("/api/invoke", post(invoke))
        .route("/api/stream", post(stream))
        .with_state(AppState { cspace })
}

async fn index() -> impl IntoResponse {
    Html(include_str!("../../frontend/index.html"))
}

async fn list_caps(State(state): State<AppState>) -> Json<Vec<CapInfo>> {
    Json(
        state
            .cspace
            .enumerate()
            .into_iter()
            .map(|m| CapInfo {
                name: m.name.clone(),
                id: m.id.to_string(),
                streaming: m.streaming,
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

    match cap.invoke_dyn(req.input) {
        Ok(value) => Ok(Json(InvokeResp { capability: req.capability, value })),
        Err(e) => Err(Json(ErrorResp { error: e })),
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
    ctx: Context,
    cspace: CapabilitySpace,
    shutdown: impl Future<Output = ()> + Send + 'static,
) {
    let app = router(ctx, cspace);
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    eprintln!("[http] listening on http://{addr}");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
        .expect("serve");
    eprintln!("[http] shut down");
}