//! Slow plugin — sleeps longer than its declared budget to exercise
//! the timeout enforcement in `Capability::invoke`.

use std::sync::Arc;
use std::time::Duration;

use cordis::{plugin_with, Context, Injection, LogLevel, Plugin};
use serde_json::Value;

use crate::kernel::{Resource, Slot};

const SLEEP: Duration = Duration::from_millis(200);

pub struct SlowResource;

impl Resource for SlowResource {
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
        vec![Injection::from("slot:slow")],
        |ctx: Context, _cfg: ()| async move {
            let slot: Arc<Slot<SlowResource>> = ctx.require("slot:slow")?;
            ctx.logger().log(
                LogLevel::Info,
                format!(
                    "slow plugin: slot={} cap_id={} timeout={}ms",
                    slot.id().raw(),
                    slot.capability()
                        .map(|c| c.id().to_string())
                        .unwrap_or_else(|| "(empty)".to_string()),
                    slot.meta().map(|m| m.timeout_ms).unwrap_or(0),
                ),
            );
            Ok(())
        },
    )
}