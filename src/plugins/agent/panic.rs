//! Panic-payload formatter for the agent's streaming path.
//!
//! Phase 4 review loop (panic containment): the streaming
//! dispatch path wraps each capability invocation in
//! `std::panic::catch_unwind` so a handler-side panic
//! surfaces as a `step_fail` event instead of unwinding
//! through the `tokio::spawn` boundary. The catch returns
//! `Box<dyn Any + Send>` on `Err`; this formatter does
//! best-effort conversion to a `String` for the failure
//! reason.
//!
//! The common payloads are `&'static str` (from `panic!("...")`)
//! and `String` (from `panic!("{}", x)`). Anything else falls
//! back to a fixed string so we always have a non-empty reason.

pub(crate) fn panic_payload_to_str(p: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = p.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_string()
    }
}
