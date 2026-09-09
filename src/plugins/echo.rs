//! Echo plugin — possession of `Slot<EchoResource, SyncKind>`.

use std::sync::Arc;

use cordis::{plugin_with, Context, Injection, LogLevel, Plugin};
use serde_json::Value;

use crate::capability::{SyncKind, SyncResource};

/// Echo is a pass-through: invoke returns its input unchanged.
pub struct EchoResource;

impl SyncResource for EchoResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        Ok(input)
    }
}

pub fn handler() -> Arc<EchoResource> {
    Arc::new(EchoResource)
}

pub fn echo_plugin() -> Arc<dyn Plugin> {
    plugin_with(
        "echo",
        vec![Injection::from("slot:echo")],
        |ctx: Context, _cfg: ()| async move {
            let slot: Arc<crate::capability::Slot<EchoResource, SyncKind>> = ctx.require("slot:echo")?;
            ctx.logger().log(
                LogLevel::Info,
                format!(
                    "echo plugin: slot={} cap_id={} timeout={}ms",
                    slot.id().raw(),
                    slot.capability()
                        .map(|c| c.id().to_string())
                        .unwrap_or_else(|| "(empty)".to_string()),
                    slot.meta().map(|m| m.timeout_ms).unwrap_or(0),
                )
                .into(),
            );
            Ok(())
        },
    )
}