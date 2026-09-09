//! Echo-chain plugin — wraps the echo token, returns a chained envelope.
//!
//! The handler factory takes an `Arc<CapabilityToken>` for echo so the
//! composite capability can delegate to its dependency at runtime. main.rs
//! is responsible for minting echo first and passing it here.

use std::sync::Arc;

use cordis::{plugin_with, Context, Injection, LogLevel, Plugin};
use serde_json::Value;

use crate::capability::{CapabilityService, CapabilityToken, SyncInvoke};

struct EchoChainHandler {
    echo: Arc<CapabilityToken>,
}

impl SyncInvoke for EchoChainHandler {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let inner = self.echo.invoke(input)?;
        Ok(serde_json::json!({
            "chained_from": self.echo.name(),
            "input": inner,
        }))
    }
}

/// Build the chain handler. `echo` must be a valid (already-minted) token;
/// main.rs guarantees ordering.
pub fn handler(echo: Arc<CapabilityToken>) -> Arc<dyn SyncInvoke> {
    Arc::new(EchoChainHandler { echo })
}

pub fn echo_chain_plugin() -> Arc<dyn Plugin> {
    plugin_with(
        "echo-chain",
        vec![
            Injection::from("cap:echo"),
            Injection::from("cap:echo_chain"),
            Injection::from("capability_service"),
        ],
        |ctx: Context, _cfg: ()| async move {
            let echo_cap: Arc<CapabilityToken> = ctx.require("cap:echo")?;
            let chain_token: Arc<CapabilityToken> = ctx.require("cap:echo_chain")?;
            let cap_svc: Arc<CapabilityService> = ctx.require("capability_service")?;
            cap_svc.register(chain_token)?;

            ctx.logger().log(
                LogLevel::Info,
                format!(
                    "echo-chain plugin: activated, depends on echo cap id={}",
                    echo_cap.id()
                )
                .into(),
            );
            Ok(())
        },
    )
}