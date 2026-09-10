//! ζ.17–ζ.20 — Phase 3 P3.3 Real AI Resources tests.
//!
//! Verify the generator plugin's model abstraction:
//!
//! - **ζ.17** — `MockModel` reproduces the historical canned
//!   behaviour for back-compat.
//! - **ζ.18** — `MarkovModel` generates non-trivial output
//!   that varies with the prompt and is deterministic for a
//!   given (model, prompt) pair.
//! - **ζ.19** — The boot's `ModelKind::from_env` selects
//!   `Markov` by default and `Mock` when `GENERATOR_MODEL=mock`.
//! - **ζ.20** — The runtime handler streams tokens in order;
//!   both models produce a stream ending in `[end]` + `Done`.

use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use odyssey::capability::{CapabilityChunk, Resource};
use odyssey::plugins::generator::{
    handler, MarkovModel, MockModel, Model, ModelKind,
};
use serde_json::Value;

const TIMEOUT: Duration = Duration::from_secs(5);

// =========================================================================
// ζ.17 — MockModel back-compat
// =========================================================================

#[test]
fn mock_model_preserves_historical_behaviour() {
    let toks = MockModel.generate("hello", None).expect("generate ok");
    let joined = toks.join(" ");
    assert!(
        joined.starts_with("Hello!"),
        "hello prompt must produce canned hello response, got: {joined}"
    );

    let toks = MockModel.generate("explain phase 3", None).expect("generate ok");
    let joined = toks.join(" ");
    assert!(
        joined.contains("Mock generator received"),
        "non-hello must echo with prefix, got: {joined}"
    );
}

// =========================================================================
// ζ.18 — MarkovModel is non-trivial, prompt-sensitive, deterministic
// =========================================================================

#[test]
fn markov_model_extends_prompt_and_is_deterministic() {
    let m = MarkovModel::default();
    let a = m.generate("the kernel", None).unwrap();
    let b = m.generate("the kernel", None).unwrap();
    assert_eq!(a, b, "same prompt + seed → same tokens");

    // Output should not be empty and should extend past the
    // prompt (i.e. not just echo).
    assert!(a.len() > 2, "markov output should add tokens beyond the seed");
    let joined = a.join(" ");
    assert!(
        joined.len() > "the kernel".len(),
        "markov output must be longer than the prompt: {joined:?}"
    );
}

#[test]
fn markov_model_varies_with_prompt() {
    let m = MarkovModel::default();
    // Different prompts → different streams (high probability).
    let a = m.generate("the kernel", None).unwrap();
    let b = m.generate("the protocol", None).unwrap();
    let c = m.generate("the resolver", None).unwrap();
    assert_ne!(a, b, "different prompts should usually differ");
    assert_ne!(b, c, "different prompts should usually differ");
}

#[test]
fn markov_model_force_seed_forces_output() {
    let m = MarkovModel::default();
    let a = m.generate("anything", Some(42)).unwrap();
    let b = m.generate("anything", Some(42)).unwrap();
    assert_eq!(a, b, "explicit seed pins output deterministically");
    let c = m.generate("anything", Some(43)).unwrap();
    // Different seed → different output (very high probability
    // for bigram over the built-in corpus).
    assert_ne!(a, c, "different seeds should produce different outputs");
}

// =========================================================================
// ζ.19 — Boot picks model from env
// =========================================================================

#[test]
fn model_kind_default_is_markov() {
    // From an env we control (or unset). `from_env` returns
    // Markov for any value other than "mock"/"Mock"/"MOCK".
    // We test by temporarily unsetting the var.
    let saved = std::env::var("GENERATOR_MODEL").ok();
    // SAFETY: tests in this crate run on a single thread; the
    // environment mutation is scoped to this test and restored
    // before the assertion returns.
    unsafe { std::env::remove_var("GENERATOR_MODEL") };
    let kind = ModelKind::from_env();
    assert_eq!(kind, ModelKind::Markov, "default is markov");
    restore_env(saved);
}

#[test]
fn model_kind_mock_when_env_says_mock() {
    let saved = std::env::var("GENERATOR_MODEL").ok();
    unsafe { std::env::set_var("GENERATOR_MODEL", "mock") };
    let kind = ModelKind::from_env();
    assert_eq!(kind, ModelKind::Mock);
    restore_env(saved);
}

#[test]
fn model_kind_markov_when_env_says_markov() {
    let saved = std::env::var("GENERATOR_MODEL").ok();
    unsafe { std::env::set_var("GENERATOR_MODEL", "markov") };
    let kind = ModelKind::from_env();
    assert_eq!(kind, ModelKind::Markov);
    restore_env(saved);
}

/// Restore the saved env var (or remove it if it was unset).
fn restore_env(saved: Option<String>) {
    // SAFETY: scoped to the same single-thread test as the
    // corresponding `set_var`/`remove_var` above.
    match saved {
        Some(v) => unsafe { std::env::set_var("GENERATOR_MODEL", v) },
        None => unsafe { std::env::remove_var("GENERATOR_MODEL") },
    }
}

// =========================================================================
// ζ.20 — Runtime handler streams in order, ends with [end] + Done
// =========================================================================

#[tokio::test]
async fn runtime_handler_streams_mock_model_in_order() {
    let model: Arc<dyn Model> = Arc::new(MockModel);
    let resource = handler(model);
    let mut rx = resource
        .open(Value::String("hello world".into()))
        .expect("open ok");

    let mut chunks: Vec<CapabilityChunk> = Vec::new();
    let collect = async {
        loop {
            match tokio::time::timeout(Duration::from_secs(2), rx.recv()).await {
                Ok(Some(c)) => chunks.push(c),
                Ok(None) => break,
                Err(_) => panic!("mock stream timed out — should be fast"),
            }
        }
    };
    let _ = tokio::time::timeout(TIMEOUT, collect).await;

    // Last two chunks must be the `[end]` marker and `Done`.
    assert!(chunks.len() >= 3, "at least one token + [end] + Done");
    let last_two = &chunks[chunks.len() - 2..];
    match &last_two[0] {
        CapabilityChunk::Item(v) => assert_eq!(v.as_str(), Some("[end]")),
        other => panic!("expected [end] item, got {other:?}"),
    }
    assert!(matches!(last_two[1], CapabilityChunk::Done));
}

#[tokio::test]
async fn runtime_handler_streams_markov_model_with_real_output() {
    let model: Arc<dyn Model> = Arc::new(MarkovModel::default());
    let resource = handler(model);
    let mut rx = resource
        .open(Value::String("the kernel".into()))
        .expect("open ok");

    // Drain the channel directly. `mpsc::Receiver::recv` returns
    // `Option<CapabilityChunk>`; stop on `Done` (which we treat
    // as a graceful close).
    let mut chunks: Vec<CapabilityChunk> = Vec::new();
    loop {
        match tokio::time::timeout(Duration::from_secs(5), rx.recv()).await {
            Ok(Some(c)) => {
                if matches!(c, CapabilityChunk::Done) {
                    chunks.push(c);
                    break;
                }
                chunks.push(c);
            }
            Ok(None) => break,
            Err(_) => panic!("markov stream timed out — model hung"),
        }
    }

    // Reconstruct the textual output (skip the final [end]
    // marker that the handler appends).
    let text = chunks
        .iter()
        .filter_map(|c| match c {
            CapabilityChunk::Item(v) => v.as_str().map(|s| s.to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ");
    assert!(text.ends_with("[end]"), "stream must end with [end], got: {text}");
    let body = text.trim_end_matches("[end]").trim();
    assert!(!body.is_empty(), "markov output body must not be empty");
    assert!(
        body.contains("kernel") || body.contains("the"),
        "markov output should contain some seed vocabulary, got: {body}"
    );
    // The final chunk in the stream must be Done.
    assert!(matches!(chunks.last(), Some(CapabilityChunk::Done)));
}