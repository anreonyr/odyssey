//! Reverse plugin — string reversal capability.

use std::sync::Arc;

use cordis::{plugin_with, Context, Injection, LogLevel, Plugin};
use serde_json::Value;

use crate::capability::{CapabilityService, CapabilityToken, SyncInvoke};

struct ReverseHandler;

impl SyncInvoke for ReverseHandler {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let s = input
            .as_str()
            .ok_or_else(|| "reverse: input must be a string".to_string())?;
        Ok(Value::String(s.chars().rev().collect()))
    }
}

pub fn handler() -> Arc<dyn SyncInvoke> {
    Arc::new(ReverseHandler)
}

pub fn reverse_plugin() -> Arc<dyn Plugin> {
    plugin_with(
        "reverse",
        vec![
            Injection::from("cap:reverse"),
            Injection::from("capability_service"),
        ],
        |ctx: Context, _cfg: ()| async move {
            let reverse_token: Arc<CapabilityToken> = ctx.require("cap:reverse")?;
            let cap_svc: Arc<CapabilityService> = ctx.require("capability_service")?;
            cap_svc.register(reverse_token.clone())?;

            ctx.logger().log(
                LogLevel::Info,
                format!("reverse plugin: activated cap id={}", reverse_token.id()).into(),
            );
            Ok(())
        },
    )
}