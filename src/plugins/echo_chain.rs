//! Echo-chain — compound resource that wraps a `Capability<EchoResource, SyncKind>`.

use std::sync::Arc;

use cordis::{plugin_with, Context, Injection, LogLevel, Plugin};
use serde_json::Value;

use crate::capability::{AnyCapability, Capability, CapabilityService, SyncKind, SyncResource};
use crate::plugins::echo::EchoResource;

pub struct EchoChainResource {
    echo: Arc<Capability<EchoResource, SyncKind>>,
}

impl SyncResource for EchoChainResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let inner = self.echo.invoke(input)?;
        Ok(serde_json::json!({
            "chained_from": self.echo.name(),
            "input": inner,
        }))
    }
}

pub fn handler(echo: Arc<Capability<EchoResource, SyncKind>>) -> Arc<EchoChainResource> {
    Arc::new(EchoChainResource { echo })
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
            let echo_cap: Arc<Capability<EchoResource, SyncKind>> = ctx.require("cap:echo")?;
            let chain_cap: Arc<Capability<EchoChainResource, SyncKind>> = ctx.require("cap:echo_chain")?;
            let cap_svc: Arc<CapabilityService> = ctx.require("capability_service")?;
            ctx.logger().log(
                LogLevel::Info,
                format!(
                    "echo-chain plugin: activated, depends on echo cap id={}",
                    echo_cap.id()
                )
                .into(),
            );
            cap_svc.register(chain_cap as Arc<dyn AnyCapability>)?;
            Ok(())
        },
    )
}