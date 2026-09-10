//! Counter resource — one shared integer behind a Mutex.
//!
//! Authority lives in the `Capability<CounterResource>` minted by the
//! factory: the resource is dumb; the kernel checks bits. The handler
//! dispatches on the JSON `{"op": "read"|"increment"|"reset"}` field
//! and trusts the caller to have gone through `invoke_op` first.
//!
//! Phase 1 invariant:
//!   * CounterResource itself never inspects authority.
//!   * Capability<CounterResource>::invoke_op(bit, ...) is the only gate.
//!   * If you can reach the handler at all, the kernel has already
//!     verified the bit.

use std::sync::Arc;
use std::sync::Mutex;

use cordis::{plugin_with, Context, Injection, LogLevel, Plugin};
use serde_json::{json, Value};

use crate::kernel::{Resource, Slot};

pub struct CounterResource {
    value: Mutex<i64>,
}

impl CounterResource {
    fn read(&self) -> i64 {
        *self.value.lock().expect("counter poisoned")
    }
    fn increment(&self) -> i64 {
        let mut g = self.value.lock().expect("counter poisoned");
        *g += 1;
        *g
    }
    fn reset(&self) -> i64 {
        let mut g = self.value.lock().expect("counter poisoned");
        *g = 0;
        *g
    }
}

impl Resource for CounterResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let op = input
            .get("op")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "counter: input must contain {\"op\": ...}".to_string())?;
        match op {
            "read" => Ok(json!({ "value": self.read() })),
            "increment" => Ok(json!({ "value": self.increment() })),
            "reset" => Ok(json!({ "value": self.reset() })),
            other => Err(format!("counter: unknown op \"{other}\"")),
        }
    }
}

pub fn handler() -> Arc<CounterResource> {
    Arc::new(CounterResource {
        value: Mutex::new(0),
    })
}

pub fn counter_plugin() -> Arc<dyn Plugin> {
    plugin_with(
        "counter",
        vec![Injection::from("slot:counter")],
        |ctx: Context, _cfg: ()| async move {
            let slot: Arc<Slot<CounterResource>> = ctx.require("slot:counter")?;
            ctx.logger().log(
                LogLevel::Info,
                format!(
                    "counter plugin: slot={} cap_id={} ops={:?}",
                    slot.id().raw(),
                    slot.capability()
                        .map(|c| c.id().to_string())
                        .unwrap_or_else(|| "(empty)".to_string()),
                    slot.capability().map(|c| c.operations()).unwrap_or_default(),
                ),
            );
            Ok(())
        },
    )
}