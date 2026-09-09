//! Reverse — typed resource `ReverseResource: SyncResource`.

use std::sync::Arc;

use cordis::{plugin_with, Context, Injection, LogLevel, Plugin};
use serde_json::Value;

use crate::capability::{AnyCapability, Capability, CapabilityService, SyncKind, SyncResource};

pub struct ReverseResource;

impl SyncResource for ReverseResource {
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
        vec![
            Injection::from("cap:reverse"),
            Injection::from("capability_service"),
        ],
        |ctx: Context, _cfg: ()| async move {
            let rev_cap: Arc<Capability<ReverseResource, SyncKind>> = ctx.require("cap:reverse")?;
            let cap_svc: Arc<CapabilityService> = ctx.require("capability_service")?;
            ctx.logger().log(
                LogLevel::Info,
                format!("reverse plugin: activated cap id={}", rev_cap.id()).into(),
            );
            cap_svc.register(rev_cap as Arc<dyn AnyCapability>)?;
            Ok(())
        },
    )
}