//! Slow plugin — sleeps longer than its declared budget to exercise timeout.

use std::sync::Arc;
use std::time::Duration;

use cordis::{plugin_with, Context, Injection, LogLevel, Plugin};
use serde_json::Value;

use crate::capability::{CapabilityService, CapabilityToken, SyncInvoke};

const SLEEP: Duration = Duration::from_millis(200);

struct SlowHandler;

impl SyncInvoke for SlowHandler {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        std::thread::sleep(SLEEP);
        Ok(input)
    }
}

pub fn handler() -> Arc<dyn SyncInvoke> {
    Arc::new(SlowHandler)
}

pub fn slow_plugin() -> Arc<dyn Plugin> {
    plugin_with(
        "slow",
        vec![
            Injection::from("cap:slow"),
            Injection::from("capability_service"),
        ],
        |ctx: Context, _cfg: ()| async move {
            let slow_token: Arc<CapabilityToken> = ctx.require("cap:slow")?;
            let cap_svc: Arc<CapabilityService> = ctx.require("capability_service")?;
            cap_svc.register(slow_token.clone())?;

            ctx.logger().log(
                LogLevel::Info,
                format!(
                    "slow plugin: activated cap id={} timeout={}ms",
                    slow_token.id(),
                    slow_token.meta().timeout_ms
                )
                .into(),
            );
            Ok(())
        },
    )
}