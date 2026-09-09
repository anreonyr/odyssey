//! Reverse plugin — `ReverseResource: Resource`. Sync only.

use std::sync::Arc;

use cordis::{plugin_with, Context, Injection, LogLevel, Plugin};
use serde_json::Value;

use crate::capability::{Resource, Slot};

pub struct ReverseResource;

impl Resource for ReverseResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let s = input
            .as_str()
            .ok_or_else(|| "reverse: input must be a string".to_string())?;
        Ok(Value::String(s.chars().rev().collect()))
    }
}

pub fn handler() -> Arc<ReverseResource> {
    Arc::new(ReverseResource)
}

pub fn reverse_plugin() -> Arc<dyn Plugin> {
    plugin_with(
        "reverse",
        vec![Injection::from("slot:reverse")],
        |ctx: Context, _cfg: ()| async move {
            let slot: Arc<Slot<ReverseResource>> = ctx.require("slot:reverse")?;
            ctx.logger().log(
                LogLevel::Info,
                format!(
                    "reverse plugin: slot={} cap_id={}",
                    slot.id().raw(),
                    slot.capability()
                        .map(|c| c.id().to_string())
                        .unwrap_or_else(|| "(empty)".to_string()),
                )
                .into(),
            );
            Ok(())
        },
    )
}