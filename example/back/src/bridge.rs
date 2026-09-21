//! HTTP bridge builtin — exposes one `CapKind::Sync` capability
//! whose Resource holds the spawned axum server.
//!
//! The plugin's `mint` reads `ODYSSEY_ADDR` (default
//! `127.0.0.1:3030`), `ODYSSEY_NO_FRONTEND`, and
//! `ODYSSEY_FRONTEND_DIST` from the environment. When no
//! frontend env is set, falls back to
//! `CARGO_MANIFEST_DIR/../../fore/dist` (mirrors the live
//! `main.rs:64-73` behavior). The server's shutdown signal
//! flows through `HttpBridgeResource`'s `Drop` —
//! `default_ruin` revokes the slot, the resource drops, the
//! cancel signal fires.
//!
//! Invoke (`/api/invoke http_bridge`) returns a status JSON
//! via `impl Resource for HttpBridgeResource` (defined in
//! `bridge_resource.rs`): `{"bound": "127.0.0.1:3030", "frontend": false}`.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use odyssey::capability::enforce::quota::CapabilityBudget;
use odyssey::core::contract::resource::Resource;
use odyssey::core::identity::ids::{PluginId, SlotId};
use odyssey::core::identity::kind::CapKind;
use odyssey::core::manifest::manifest::{CapabilityDecl, ManifestBuilder, PluginManifest};
use odyssey::core::rights::rights::{CapabilityRights, Rights};
use odyssey::personality::composition::resolve::ResolvedBinding;
use odyssey::personality::lifecycle::mint::{CapabilityFactory, MintError, TypedBindings};
use odyssey::personality::lifecycle::run::{MintFn, RuinFn, default_ruin};
use odyssey::personality::lifecycle::serve;
use tokio::sync::oneshot;

use crate::bridge_resource::HttpBridgeResource;

/// Default bind address when `ODYSSEY_ADDR` is unset.
const DEFAULT_BRIDGE_ADDR: &str = "127.0.0.1:3030";

pub struct HttpBridgeBuiltin;

impl HttpBridgeBuiltin {
    pub fn manifest() -> PluginManifest {
        // `.expose(name, contract_name)` per `manifest.rs:222-234`.
        // The contract_name is what other plugins' `requires`
        // matches against; for a self-contained bridge with no
        // requires, mirroring the plugin name keeps the contract
        // namespace flat. `ManifestBuilder::expose` is the
        // 2-arg sync variant (`expose_streaming` is separate
        // for `CapKind::Stream`).
        ManifestBuilder::new("http_bridge")
            .version("0.1.0")
            .expose("http_bridge", "http_bridge")
            .build()
    }

    pub fn mint(
        factory: &CapabilityFactory,
        plugin: &PluginId,
        decl: &CapabilityDecl,
        kind: CapKind,
        budget: CapabilityBudget,
        _bindings: &[ResolvedBinding],
        _typed_bindings: &TypedBindings,
    ) -> Result<SlotId, MintError> {
        let addr: SocketAddr = std::env::var("ODYSSEY_ADDR")
            .unwrap_or_else(|_| DEFAULT_BRIDGE_ADDR.to_string())
            .parse()
            .expect("ODYSSEY_ADDR must be host:port");

        // Mirror live `main.rs:64-73` behavior: prefer
        // `ODYSSEY_FRONTEND_DIST`, fall back to
        // `CARGO_MANIFEST_DIR/../../fore/dist`, unless
        // `ODYSSEY_NO_FRONTEND` is set (in which case the
        // server returns 503 for UI routes).
        let frontend_dist: Option<PathBuf> = if std::env::var("ODYSSEY_NO_FRONTEND").is_ok() {
            None
        } else {
            std::env::var("ODYSSEY_FRONTEND_DIST")
                .ok()
                .map(PathBuf::from)
                .or_else(|| {
                    Some(
                        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                            .parent()
                            .expect("example/backend has a parent")
                            .join("fore/dist"),
                    )
                })
        };

        let (cancel_tx, cancel_rx) = oneshot::channel::<()>();
        let server_handle = serve::spawn_http_bridge_with_shutdown(
            addr,
            factory.space().clone(),
            frontend_dist.as_deref(),
            async move {
                let _ = cancel_rx.await;
            },
        );

        let resource = Arc::new(HttpBridgeResource {
            server_handle,
            cancel_tx: Some(cancel_tx),
            bound_addr: addr,
            has_frontend: frontend_dist.is_some(),
        });

        // Slice 3 PluginCspace pattern (per
        // `agent/builtin.rs:475-506`): mint into the plugin's
        // own cspace, then grant a derived slot to the
        // global cspace. Generic over R so the
        // `pc.inner().grant_to::<R>(...)` call has its type
        // parameter bound.
        mint_cap::<HttpBridgeResource>(factory, plugin, kind, decl, budget, resource)
    }

    pub fn register() -> (PluginManifest, MintFn, RuinFn) {
        (
            Self::manifest(),
            // `HttpBridgeBuiltin` is a unit struct (no
            // instance data) so `Self::mint` is effectively a
            // static fn pointer and coerces to `MintFn`
            // directly. A closure would force higher-ranked
            // lifetime inference on `&[ResolvedBinding]` and
            // `&TypedBindings`; using the function pointer
            // sidesteps that.
            Self::mint,
            default_ruin,
        )
    }
}

/// Slice 3 helper: mint a single resource into the plugin's
/// own cspace, then grant a derived slot into the
/// orchestrator's global cspace.
///
/// Mirrors `agent/builtin.rs:475-506` — the canonical
/// precedent for the per-plugin-isolation mint flow.
/// `pc.mint` returns `SlotId` directly (no `Result`); the
/// only fallible step is `grant_to`, which can fail with
/// `AttenuationViolation` or `SlotEmpty` and is mapped to
/// `MintError::GrantFailed`.
fn mint_cap<R: Resource>(
    factory: &CapabilityFactory,
    plugin: &PluginId,
    kind: CapKind,
    decl: &CapabilityDecl,
    budget: CapabilityBudget,
    resource: Arc<R>,
) -> Result<SlotId, MintError> {
    let pc = factory.plugin_cspace(plugin);
    // Read `timeout_ms` from `budget` before passing it to
    // `pc.mint` — `mint` consumes its `budget` argument
    // (no Clone impl on `CapabilityBudget` in this crate,
    // so we can't `budget.clone()`).
    let timeout_ms = budget.timeout_ms();
    let local_slot = pc.mint(kind, decl, budget, resource);
    let rights = CapabilityRights {
        operations: Rights::INVOKE,
        timeout_ms,
    };
    pc.inner()
        .grant_to::<R>(local_slot, factory.space(), rights, decl.name.clone())
        .map_err(|e| MintError::GrantFailed {
            plugin: plugin.name.clone(),
            cap: decl.name.clone(),
            source: e,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `manifest()` produces a valid `PluginManifest` with
    /// one expose named `http_bridge` of `CapKind::Sync`.
    #[test]
    fn manifest_has_one_sync_expose() {
        let m = HttpBridgeBuiltin::manifest();
        assert_eq!(m.plugin.name, "http_bridge");
        assert_eq!(m.plugin.version, "0.1.0");
        assert_eq!(m.exposes.len(), 1);
        assert_eq!(m.exposes[0].name, "http_bridge");
        assert_eq!(m.exposes[0].kind, CapKind::Sync);
        assert_eq!(m.exposes[0].contract_name, "http_bridge");
    }

    /// `register()` returns the (manifest, mint_fn,
    /// default_ruin) triple per the project's plugin
    /// convention.
    #[test]
    fn register_returns_plugin_triple() {
        let (m, _mint, _ruin) = HttpBridgeBuiltin::register();
        assert_eq!(m.plugin.name, "http_bridge");
    }
}
