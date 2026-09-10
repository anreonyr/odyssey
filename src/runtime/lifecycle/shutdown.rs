//! HTTP bridge spawn + shutdown coordination.
//!
//! The HTTP bridge runs as a tokio task; the orchestrator awaits
//! the returned `JoinHandle`. Ctrl-C is signalled via a future
//! passed to `serve`; when Ctrl-C arrives, the bridge task
//! completes and `server_handle.await` returns.

use std::net::SocketAddr;

use crate::kernel::CapabilitySpace;

/// Spawn the HTTP bridge on `addr` and return the task handle.
///
/// The bridge runs until either side returns from the serve
/// future. We pass a Ctrl-C future to `serve` so the orchestrator
/// can shut the bridge down by simply dropping the awaiter.
pub fn spawn_http_bridge(addr: SocketAddr, cspace: CapabilitySpace) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        crate::runtime::http_bridge::serve(
            addr,
            cspace,
            async {
                let _ = tokio::signal::ctrl_c().await;
            },
        )
        .await;
    })
}
