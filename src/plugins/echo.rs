//! Echo plugin — typed resource `EchoResource: SyncResource`.

use std::sync::Arc;

use cordis::{plugin_with, Context, Injection, LogLevel, Plugin};
use serde_json::Value;

use crate::capability::{AnyCapability, Capability, CapabilityService, SyncKind, SyncResource};

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
        vec![
            Injection::from("cap:echo"),
            Injection::from("capability_service"),
        ],
        |ctx: Context, _cfg: ()| async move {
            let cap: Arc<Capability<EchoResource, SyncKind>> = ctx.require("cap:echo")?;
            let cap_svc: Arc<CapabilityService> = ctx.require("capability_service")?;
            ctx.logger().log(
                LogLevel::Info,
                format!(
                    "echo plugin: activated cap id={} timeout={}ms",
                    cap.id(),
                    cap.meta().timeout_ms
                )
                .into(),
            );
            cap_svc.register(cap as Arc<dyn AnyCapability>)?;
            Ok(())
        },
    )
}