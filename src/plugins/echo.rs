//! Echo plugin — owns its handler, registers its own token.
//!
//! The handler implementation lives in this module; `main.rs` calls
//! `echo::handler()` to obtain it before minting the capability token. The
//! plugin body itself only receives the minted token and registers it with
//! the capability service.

use std::sync::Arc;

use cordis::{plugin_with, Context, Injection, LogLevel, Plugin};
use serde_json::Value;

use crate::capability::{CapabilityService, CapabilityToken, SyncInvoke};

struct EchoHandler;

impl SyncInvoke for EchoHandler {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        Ok(input)
    }
}

/// The handler implementation, owned by this plugin module.
pub fn handler() -> Arc<dyn SyncInvoke> {
    Arc::new(EchoHandler)
}

/// Plugin fiber. Declares `cap:echo` in inject so the fiber waits for
/// main.rs to mint and provide the token before activating.
pub fn echo_plugin() -> Arc<dyn Plugin> {
    plugin_with(
        "echo",
        vec![
            Injection::from("cap:echo"),
            Injection::from("capability_service"),
        ],
        |ctx: Context, _cfg: ()| async move {
            let cap_token: Arc<CapabilityToken> = ctx.require("cap:echo")?;
            let cap_svc: Arc<CapabilityService> = ctx.require("capability_service")?;
            cap_svc.register(cap_token.clone())?;

            ctx.logger().log(
                LogLevel::Info,
                format!(
                    "echo plugin: activated cap id={} timeout={}ms",
                    cap_token.id(),
                    cap_token.meta().timeout_ms
                )
                .into(),
            );
            Ok(())
        },
    )
}