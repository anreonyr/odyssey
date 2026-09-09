//! Slow — sleeps longer than its declared budget to exercise timeout.

use std::sync::Arc;
use std::time::Duration;

use cordis::{plugin_with, Context, Injection, LogLevel, Plugin};
use serde_json::Value;

use crate::capability::{AnyCapability, Capability, CapabilityService, SyncKind, SyncResource};

const SLEEP: Duration = Duration::from_millis(200);

pub struct SlowResource;

impl SyncResource for SlowResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        std::thread::sleep(SLEEP);
        Ok(input)
    }
}

pub fn handler() -> Arc<SlowResource> {
    Arc::new(SlowResource)
}

pub fn slow_plugin() -> Arc<dyn Plugin> {
    plugin_with(
        "slow",
        vec![
            Injection::from("cap:slow"),
            Injection::from("capability_service"),
        ],
        |ctx: Context, _cfg: ()| async move {
            let slow_cap: Arc<Capability<SlowResource, SyncKind>> = ctx.require("cap:slow")?;
            let cap_svc: Arc<CapabilityService> = ctx.require("capability_service")?;
            ctx.logger().log(
                LogLevel::Info,
                format!(
                    "slow plugin: activated cap id={} timeout={}ms",
                    slow_cap.id(),
                    slow_cap.meta().timeout_ms
                )
                .into(),
            );
            cap_svc.register(slow_cap as Arc<dyn AnyCapability>)?;
            Ok(())
        },
    )
}