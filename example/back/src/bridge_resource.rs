//! `HttpBridgeResource` — Resource type for the HTTP bridge plugin.
//!
//! The capability-kernel's slot-revoke path drops this resource,
//! which sends the cancel signal — graceful shutdown flows
//! through `axum::serve().with_graceful_shutdown()`.
//!
//! `bound_addr` and `has_frontend` are recorded at mint time
//! for the `Resource::invoke` status response — the capability
//! returns server status when invoked via `/api/invoke http_bridge`.
//!
//! Slice 1 ships the struct + Drop. Slice 2's
//! `HttpBridgeBuiltin::mint` spawns the server via
//! `serve::spawn_http_bridge_with_shutdown` and constructs the
//! resource via `HttpBridgeResource::new`.

use std::net::SocketAddr;

use odyssey::core::contract::resource::Resource;
use serde_json::{Value, json};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// Resource wrapping the HTTP bridge's lifecycle.
///
/// Drop sends the cancel signal to the spawned server task
/// via `oneshot`. The receiver side was moved into the
/// spawned task as the shutdown future for
/// `serve::spawn_http_bridge_with_shutdown`.
pub struct HttpBridgeResource {
    /// Handle to the spawned axum server task. Held so the
    /// `JoinHandle` doesn't drop (which would detach the
    /// task silently). Cancellation goes through `cancel_tx`
    /// instead.
    pub server_handle: JoinHandle<()>,
    /// Sender for the shutdown signal. Wrapped in `Option`
    /// so `Drop` can `take()` it out of `&mut self` —
    /// `oneshot::Sender::send` takes `self` by value. The
    /// spawned task's shutdown future receives.
    pub cancel_tx: Option<oneshot::Sender<()>>,
    /// Bound address — recorded at mint time for the
    /// `Resource::invoke` status response.
    pub bound_addr: SocketAddr,
    /// Whether a frontend dist was configured at mint time.
    pub has_frontend: bool,
}

impl HttpBridgeResource {
    /// Construct a resource from already-spawned pieces.
    ///
    /// Slice 2's `HttpBridgeBuiltin::mint` does the spawning
    /// via `serve::spawn_http_bridge_with_shutdown` and passes
    /// the resulting `JoinHandle` + `oneshot::Sender` here.
    /// Splitting construction from spawning keeps `bridge_resource.rs`
    /// free of kernel-import paths — the file lives in the
    /// example layer (`odyssey-builtin`), so it must use
    /// `odyssey::...` for any kernel reference; the spawn
    /// happens in `bridge.rs` (Slice 2) where the import is
    /// already in scope.
    pub fn new(
        server_handle: JoinHandle<()>,
        cancel_tx: oneshot::Sender<()>,
        bound_addr: SocketAddr,
        has_frontend: bool,
    ) -> Self {
        Self {
            server_handle,
            cancel_tx: Some(cancel_tx),
            bound_addr,
            has_frontend,
        }
    }
}

impl Drop for HttpBridgeResource {
    fn drop(&mut self) {
        // Send the cancel signal. The receiver was already
        // moved into the spawned task; if the task already
        // finished, this is a no-op (channel closed). Use
        // `Option::take` to move out of the `&mut self`
        // reference — `Sender::send` takes `self` by value.
        if let Some(tx) = self.cancel_tx.take() {
            let _ = tx.send(());
        }
    }
}

impl Resource for HttpBridgeResource {
    /// `Resource::invoke` — returns the bridge server status
    /// as JSON. This is the function the kernel calls when
    /// `/api/invoke http_bridge` is hit. The actual server
    /// lifecycle is held by the spawned tokio task; this
    /// `invoke` body is informational only.
    fn invoke(&self, _input: Value) -> Result<Value, String> {
        Ok(json!({
            "bound": self.bound_addr.to_string(),
            "frontend": self.has_frontend,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke test: construct the resource via `new`, verify
    /// fields are accessible, then drop. Real cancellation
    /// behavior is exercised by the integration test
    /// (`example/fore/test/frontend.test.tsx` driving the
    /// live binary with Ctrl-C).
    #[tokio::test]
    async fn struct_constructs_and_drops() {
        let handle = tokio::spawn(async {});
        let (tx, _rx) = oneshot::channel::<()>();
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let resource = HttpBridgeResource::new(handle, tx, addr, false);
        assert_eq!(resource.bound_addr.port(), 0);
        assert!(!resource.has_frontend);
        // Drop fires the cancel signal; receiver was already
        // dropped by end-of-scope of `_rx`, so `tx.send`
        // returns `Err(SendError)` which Drop ignores.
        drop(resource);
    }

    /// Verify `has_frontend` records the configured state.
    #[tokio::test]
    async fn new_records_frontend_flag() {
        let handle = tokio::spawn(async {});
        let (tx, _rx) = oneshot::channel::<()>();
        let resource = HttpBridgeResource::new(handle, tx, "127.0.0.1:0".parse().unwrap(), true);
        assert!(resource.has_frontend);
        drop(resource);
    }
}
