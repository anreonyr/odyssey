//! Personality / lifecycle / serve — HTTP bridge for capability dispatch.
//!
//! Phase 8: merged the Phase 5 split (`runtime/http_bridge.rs`
//! for the axum router + `runtime/lifecycle/shutdown.rs` for
//! the spawn wrapper) into one cohesive module.
//!
//! Routes:
//!
//! - `GET  /`            → React app entry (`dist/index.html`)
//! - `GET  /assets/*`    → React app's hashed bundle
//! - `GET  /api/caps`    → list capabilities
//! - `POST /api/invoke`  → invoke a sync capability
//! - `POST /api/stream`  → open a streaming capability (SSE)

use std::convert::Infallible;
use std::future::Future;
use std::net::SocketAddr;
use std::path::PathBuf;

use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::{
        Html, IntoResponse, Json, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;

use crate::capability::enforce::space::CapabilitySpace;
use crate::core::identity::kind::CapKind;
use crate::core::meta::chunk::CapabilityChunk;
use crate::core::meta::meta::CapabilityMeta;

/// Optional path to the built React app (`dist/` produced by
/// `pnpm --dir example/frontend build`). When `None`, the
/// `/`, `/assets/*`, and `.fallback` routes return 503 —
/// callers who only want the API surface (a unit test, say)
/// don't need a frontend. When `Some`, `serve` reads the
/// bundle from disk so the library stays UI-free (no
/// `include_str!` of HTML, no embed-time binding).
///
/// The library intentionally does not derive this from
/// `CARGO_MANIFEST_DIR` — it used to, when the example was
/// `examples/basic.rs` at the workspace root. The example
/// now lives at `example/backend/`, two directories away
/// from `example/frontend/`, so a relative path would have
/// to walk back through `../..` and stay correct under
/// `cargo publish`. Passing it in keeps `serve` decoupled
/// from where the example binary happens to live.
#[derive(Clone)]
struct AppState {
    cspace: CapabilitySpace,
    frontend_dist: Option<PathBuf>,
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

#[derive(Serialize)]
struct CheckpointInfo {
    path: String,
    session_id: String,
    saved_at: u64,
    size: u64,
}

pub fn router(cspace: CapabilitySpace, frontend_dist: Option<&std::path::Path>) -> Router {
    Router::new()
        .route("/", get(index))
        // SPA fallback — any GET that doesn't match an asset or
        // an API endpoint returns the React app's index.html. The
        // client-side router (BrowserRouter) takes it from there.
        // /assets/* and /api/* are listed first so they take
        // precedence; the wildcard below only catches paths the
        // router hasn't already matched.
        .route("/assets/*path", get(static_asset))
        .route("/api/caps", get(list_caps))
        .route("/api/invoke", post(invoke))
        .route("/api/stream", post(stream))
        .route("/api/checkpoints", get(list_checkpoints))
        .fallback(get(index))
        .with_state(AppState {
            cspace,
            frontend_dist: frontend_dist.map(std::path::Path::to_path_buf),
        })
}

/// Reads the React app's `index.html` from disk. The build
/// pipeline (`pnpm --dir example/frontend build`) writes
/// it next to the example binary; we don't embed it so the
/// library can stay free of UI assets. When the example
/// binary doesn't pass a `frontend_dist`, we return 503
/// rather than synthesising a stub — the absence is a
/// configuration choice, not a broken build.
async fn index(State(state): State<AppState>) -> Response {
    let Some(dist) = state.frontend_dist.as_deref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "frontend dist not configured for this server".to_string(),
        )
            .into_response();
    };
    let path = dist.join("index.html");
    match std::fs::read_to_string(&path) {
        Ok(html) => Html(html).into_response(),
        Err(e) => (
            StatusCode::SERVICE_UNAVAILABLE,
            format!(
                "React app not built. Run `pnpm --dir example/frontend build` \
                 first. ({e})"
            ),
        )
            .into_response(),
    }
}

/// Serves files under `dist/assets/*` — Vite emits a hashed
/// filename per asset, so we serve anything the assets directory
/// contains rather than maintaining an explicit list. 404s are
/// honest: a stale index.html asking for a missing hash means
/// the cache and the build drifted, not that the route is wrong.
async fn static_asset(
    State(state): State<AppState>,
    Path(path): Path<String>,
) -> Response {
    // `path` is the wildcard tail after `/assets/`; reject
    // directory traversal by stripping leading `/`s and refusing
    // any `..` segment.
    let safe = path.trim_start_matches('/');
    if safe.is_empty() || safe.contains("..") {
        return (StatusCode::BAD_REQUEST, "bad path").into_response();
    }
    let Some(dist) = state.frontend_dist.as_deref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "frontend dist not configured for this server".to_string(),
        )
            .into_response();
    };
    let full = dist.join("assets").join(safe);
    match std::fs::read(&full) {
        Ok(bytes) => bytes.into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "asset not found").into_response(),
    }
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

/// Enumerate checkpoint files under `ODYSSEY_CHECKPOINT_DIR` (or
/// the default `./.checkpoints/` if unset). Each file is a
/// `SessionCheckpoint` JSON the agent_runtime writes from
/// `cancel(path=...)`; we read enough of it to surface
/// `session_id` + `saved_at` + size, then filter out anything
/// that failed to parse so a single corrupt file can't take the
/// page down.
///
/// The directory is intentionally not auto-created: if it
/// doesn't exist, no checkpoints have been written yet, so we
/// return `[]` rather than 404. (Page treats `[]` as empty
/// state.) Files that vanish between readdir and stat are
/// skipped silently — the listing is a snapshot, not a lock.
async fn list_checkpoints() -> Json<Vec<CheckpointInfo>> {
    use serde_json::Value;

    let dir = std::env::var("ODYSSEY_CHECKPOINT_DIR")
        .unwrap_or_else(|_| "./.checkpoints".to_string());
    let path = std::path::Path::new(&dir);
    if !path.is_dir() {
        return Json(Vec::new());
    }

    let mut out = Vec::new();
    let entries = match std::fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => return Json(Vec::new()),
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let Ok(meta) = std::fs::metadata(&p) else { continue };
        let Ok(bytes) = std::fs::read(&p) else { continue };
        // The checkpoint envelope has many fields; we only
        // surface `id` (session_id) and `created_at_ms` (ms
        // epoch → converted to seconds for the page). Anything
        // we can't parse we drop — see the module doc above.
        let Ok(v) = serde_json::from_slice::<Value>(&bytes) else { continue };
        let Some(session_id) = v.get("id").and_then(|x| x.as_str()) else { continue };
        let saved_at_ms = v
            .get("created_at_ms")
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
        out.push(CheckpointInfo {
            path: p.to_string_lossy().into_owned(),
            session_id: session_id.to_string(),
            saved_at: saved_at_ms / 1000,
            size: meta.len(),
        });
    }
    // Newest first so the page's first row is the most recent save.
    out.sort_by(|a, b| b.saved_at.cmp(&a.saved_at));
    Json(out)
}

async fn invoke(
    State(state): State<AppState>,
    Json(req): Json<InvokeReq>,
) -> Result<Json<InvokeResp>, Json<ErrorResp>> {
    let cap = state
        .cspace
        .lookup_by_name(&req.capability)
        .ok_or_else(|| {
            Json(ErrorResp {
                error: format!("capability not found: {}", req.capability),
            })
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
        Ok(value) => Ok(Json(InvokeResp {
            capability: req.capability,
            value,
        })),
        Err(e) => Err(Json(ErrorResp {
            error: e.to_string(),
        })),
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
                            CapabilityChunk::Done => Ok(Event::default().event("done").data("")),
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

    Sse::new(tokio_stream::wrappers::ReceiverStream::new(event_rx)).keep_alive(KeepAlive::default())
}

pub async fn serve(
    addr: SocketAddr,
    cspace: CapabilitySpace,
    frontend_dist: Option<&std::path::Path>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) {
    let app = router(cspace, frontend_dist);
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[http] bind {addr} failed: {e}");
            return;
        }
    };
    eprintln!("[http] listening on http://{addr}");
    if let Err(e) = axum::serve(listener, app).with_graceful_shutdown(shutdown).await {
        eprintln!("[http] serve error: {e}");
    }
    eprintln!("[http] shut down");
}

/// Spawn the HTTP bridge on `addr` and return the task handle.
///
/// The bridge runs until either side returns from the serve
/// future. We pass a Ctrl-C future to `serve` so the orchestrator
/// can shut the bridge down by simply dropping the awaiter.
pub fn spawn_http_bridge(
    addr: SocketAddr,
    cspace: CapabilitySpace,
    frontend_dist: Option<&std::path::Path>,
) -> tokio::task::JoinHandle<()> {
    let frontend_dist = frontend_dist.map(std::path::Path::to_path_buf);
    tokio::spawn(async move {
        serve(addr, cspace, frontend_dist.as_deref(), async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await;
    })
}
