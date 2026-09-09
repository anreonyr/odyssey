//! Echo-chain — compound `Slot<EchoChainResource, SyncKind>` whose
//! `SyncResource` impl delegates to a `Capability<EchoResource, SyncKind>`
//! it was handed at construction.

use std::sync::Arc;

use cordis::{plugin_with, Context, Injection, LogLevel, Plugin};
use serde_json::Value;

use crate::capability::{Capability, SyncKind, SyncResource};
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
            Injection::from("slot:echo"),
            Injection::from("slot:echo_chain"),
        ],
        |ctx: Context, _cfg: ()| async move {
            let echo_slot: Arc<crate::capability::Slot<EchoResource, SyncKind>> =
                ctx.require("slot:echo")?;
            let _chain_slot: Arc<crate::capability::Slot<EchoChainResource, SyncKind>> =
                ctx.require("slot:echo_chain")?;
            ctx.logger().log(
                LogLevel::Info,
                format!(
                    "echo-chain plugin: depends on echo slot {} (cap={})",
                    echo_slot.id().raw(),
                    echo_slot
                        .capability()
                        .map(|c| c.id().to_string())
                        .unwrap_or_else(|| "(empty)".to_string()),
                )
                .into(),
            );
            Ok(())
        },
    )
}