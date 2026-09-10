//! Phase 3 P3.3 — Real AI resources.
//!
//! The `generator` plugin used to be a **mock**: detect "hello"
//! in the prompt and emit a canned response, otherwise echo
//! the prompt. P3.3 replaces that with a `Model` trait and two
//! implementations:
//!
//! - [`MockModel`] — keeps the canned-response behaviour for
//!   back-compat and for tests that want deterministic
//!   canned output.
//! - [`MarkovModel`] — a real n-gram generator trained on a
//!   built-in tech/programming corpus. Different prompts
//!   produce different, locally-coherent output. Deterministic
//!   given a seed (handy for tests).
//!
//! The boot pipeline selects which model to install via the
//! `GENERATOR_MODEL` env var (`mock` or `markov`, default
//! `markov`). Tests that need specific behaviour pass the
//! model directly to [`handler::handler`].
//!
//! ## Why not a real LLM?
//!
//! A "real" LLM call would mean network I/O, an API key
//! dependency, and non-determinism that breaks the
//! streaming-quota / quota-spec test surface. A Markov chain
//! hits the same shape (variable-length stream of tokens
//! that depends on the prompt) without those costs. The
//! [`Model`] trait is what `generator`'s handler dispatches
//! through, so swapping in an HTTP-bridged LLM later is a
//! one-file change.
//!
//! ## Determinism
//!
//! [`MarkovModel`] uses a `StdRng` seeded by `prompt.hash()`
//! XOR'd with the model's own seed. Same `(model, prompt)`
//! pair → same output stream. Tests can pin the output.

use std::collections::HashMap;

use rand::rngs::StdRng;
use rand::SeedableRng;
use rand::seq::SliceRandom;
use serde_json::Value;

/// The model abstraction. `generator`'s handler dispatches
/// through this trait; concrete implementations are
/// [`MockModel`] and [`MarkovModel`]. Future implementations
/// (HTTP-bridged LLM, ONNX runtime, ...) plug in here.
///
/// All methods are sync — the handler runs `generate` on a
/// `spawn_blocking` task and streams the resulting tokens
/// back over an `mpsc::channel` at its own pace.
pub trait Model: Send + Sync {
    /// Generate a stream of tokens for `prompt`. Each token is
    /// emitted as a string; the handler wraps it in a
    /// `CapabilityChunk::Item`. The implementation picks the
    /// tokenisation unit — character, sub-word, word —
    /// independently of any external caller.
    ///
    /// `seed` lets callers force determinism without
    /// depending on prompt content. `None` means "use the
    /// model's own seed".
    fn generate(&self, prompt: &str, seed: Option<u64>) -> Result<Vec<String>, String>;
}

/// Back-compat: the canned-response behaviour the original
/// `generator` had. Keeps the same surface so existing tests
/// don't change behaviour.
pub struct MockModel;

impl Model for MockModel {
    fn generate(&self, prompt: &str, _seed: Option<u64>) -> Result<Vec<String>, String> {
        let response = if prompt.to_lowercase().contains("hello") {
            "Hello! I'm a mock generator — the framework supports any provider.".to_string()
        } else {
            format!("Mock generator received: \"{prompt}\"")
        };
        // Match the historical behaviour: chunk by whitespace so
        // the streaming shape stays close to a real generator.
        Ok(response.split_whitespace().map(|s| s.to_string()).collect())
    }
}

/// Order of the n-gram model. Bigger `n` means more local
/// coherence (sentences read more naturally) but requires a
/// larger training corpus. Default 2 (bigram) works for the
/// built-in corpus.
pub type NGramOrder = usize;

/// A real n-gram generator. Trained on a built-in corpus of
/// tech / programming vocabulary (see [`DEFAULT_CORPUS`]);
/// produces tokens by walking the chain from a seed word.
///
/// Construction:
/// - [`MarkovModel::default()`] — uses the built-in corpus,
///   seed = 0, bigram.
/// - [`MarkovModel::with_corpus(corpus, order, seed)`] —
///   custom corpus / order / seed.
pub struct MarkovModel {
    /// n-gram table: `(prefix tuple) → (suffix → count)`.
    table: HashMap<Vec<String>, Vec<String>>,
    /// Order of the n-gram (2 = bigram, 3 = trigram).
    order: NGramOrder,
    /// Seed for the RNG. Combined with `prompt.hash()` for
    /// determinism given a prompt.
    seed: u64,
}

impl Default for MarkovModel {
    fn default() -> Self {
        Self::with_corpus(DEFAULT_CORPUS.iter().map(|s| s.to_string()).collect(), 2, 0)
    }
}

impl MarkovModel {
    /// Build a model from a flat token list (the corpus
    /// pre-tokenised). `order` is the n-gram order (typical:
    /// 2 for bigram, 3 for trigram).
    pub fn with_corpus(corpus: Vec<String>, order: NGramOrder, seed: u64) -> Self {
        let order = order.max(1);
        let mut table: HashMap<Vec<String>, Vec<String>> = HashMap::new();
        // Slide a window of size `order + 1` over the corpus,
        // each window contributes one (prefix → suffix) row.
        // Use a sentinel `<END>` so the model knows when to
        // stop without explicit length tracking.
        let extended: Vec<String> = corpus
            .into_iter()
            .chain(std::iter::once(END_SENTINEL.to_string()))
            .collect();
        for window in extended.windows(order + 1) {
            let prefix = window[..order].to_vec();
            let suffix = window[order].clone();
            table.entry(prefix).or_default().push(suffix);
        }
        Self { table, order, seed }
    }

    /// Number of unique n-grams in the table. Useful for
    /// diagnostics and tests.
    pub fn ngram_count(&self) -> usize {
        self.table.len()
    }

    /// Generate up to `max_tokens` tokens starting from
    /// `prompt`. The first `self.order` tokens of `prompt`
    /// seed the chain. If the seed isn't in the table, we
    /// fall back to a random starting prefix.
    pub fn sample(
        &self,
        prompt: &str,
        max_tokens: usize,
        rng: &mut StdRng,
    ) -> Vec<String> {
        let prompt_tokens: Vec<String> =
            prompt.split_whitespace().map(|s| s.to_string()).collect();
        let prefix: Vec<String> = if prompt_tokens.len() >= self.order {
            prompt_tokens[..self.order].to_vec()
        } else {
            // Pad with start-sentinel tokens so the chain has
            // something to look up.
            let pad = vec![START_SENTINEL.to_string(); self.order - prompt_tokens.len()];
            pad.into_iter().chain(prompt_tokens.into_iter()).collect()
        };

        let mut out: Vec<String> = prefix.clone();
        let lookup_prefix = prefix.clone();

        // Choose a starting prefix the table actually has,
        // preferring one that overlaps the prompt's first word
        // for "prompt-aware" output.
        let mut current = if self.table.contains_key(&lookup_prefix) {
            prefix.clone()
        } else {
            // Fall back: pick a random starting prefix.
            let keys: Vec<&Vec<String>> = self.table.keys().collect();
            if keys.is_empty() {
                return out;
            }
            let chosen = keys.choose(rng).copied().cloned().unwrap_or(prefix.clone());
            out = chosen.clone();
            chosen
        };

        for _ in 0..max_tokens {
            let suffixes = match self.table.get(&current) {
                Some(s) if !s.is_empty() => s,
                _ => break,
            };
            let next = suffixes.choose(rng).cloned().unwrap_or_else(|| END_SENTINEL.to_string());
            if next == END_SENTINEL {
                break;
            }
            out.push(next.clone());
            // Slide the window.
            current = current.iter().skip(1).cloned().chain(std::iter::once(next)).collect();
        }
        out
    }
}

impl Model for MarkovModel {
    fn generate(&self, prompt: &str, seed: Option<u64>) -> Result<Vec<String>, String> {
        // Determinism: combine prompt hash with model seed.
        let prompt_hash: u64 = {
            let mut h: u64 = 14695981039346656037; // FNV-1a basis
            for b in prompt.bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(1099511628211);
            }
            h
        };
        let combined_seed = seed.unwrap_or(self.seed).wrapping_add(prompt_hash);
        let mut rng = StdRng::seed_from_u64(combined_seed);
        Ok(self.sample(prompt, MAX_TOKENS, &mut rng))
    }
}

const START_SENTINEL: &str = "<START>";
const END_SENTINEL: &str = "<END>";
const MAX_TOKENS: usize = 64;

/// Built-in training corpus. Tech / programming vocabulary,
/// shaped to produce locally-coherent short sentences when
/// walked as a bigram. Kept small (≈100 tokens) so the model
/// is fast to construct and the test surface is stable.
pub const DEFAULT_CORPUS: &[&str] = &[
    "the", "kernel", "dispatches", "calls", "through", "the", "capability", "space",
    "and", "checks", "authority", "before", "the", "handler", "runs",
    "the", "agent", "binds", "to", "the", "echo", "capability", "via", "the",
    "binding", "table", "produced", "by", "the", "resolver",
    "the", "resolver", "walks", "the", "manifest", "graph", "in", "topological",
    "order", "and", "records", "every", "binding", "the", "consumer", "needs",
    "the", "runtime", "starts", "plugins", "in", "mint", "order", "and", "stops",
    "them", "in", "reverse", "mint", "order",
    "the", "shutdown", "phase", "revokes", "every", "slot", "the", "plugin",
    "owns", "so", "the", "cspace", "ends", "empty",
    "the", "protocol", "is", "metadata", "only", "and", "does", "not", "drive",
    "the", "invoke", "path",
    "the", "graph", "event", "bus", "broadcasts", "every", "mutation", "so",
    "subscribers", "see", "the", "full", "timeline",
    "the", "agent", "consults", "the", "authority", "contract", "to", "translate",
    "actions", "to", "operation", "rights", "and", "the", "kernel", "enforces",
    "the", "bits",
];

// ---------------------------------------------------------------------------
// Selection: which model does the boot install?
// ---------------------------------------------------------------------------

/// Identify a model by name. Case-insensitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelKind {
    Mock,
    Markov,
}

impl ModelKind {
    /// Parse from the `GENERATOR_MODEL` env var. Returns the
    /// default (`Markov`) if unset or unrecognised. Logging
    /// the unrecognised value is the caller's responsibility.
    pub fn from_env() -> Self {
        match std::env::var("GENERATOR_MODEL").ok().as_deref() {
            Some("mock") | Some("Mock") | Some("MOCK") => Self::Mock,
            _ => Self::Markov,
        }
    }

    /// Build the matching `Model` instance.
    pub fn build(self) -> Box<dyn Model> {
        match self {
            Self::Mock => Box::new(MockModel),
            Self::Markov => Box::new(MarkovModel::default()),
        }
    }
}

impl std::str::FromStr for ModelKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "mock" => Ok(Self::Mock),
            "markov" => Ok(Self::Markov),
            other => Err(format!("unknown model kind '{other}'; expected 'mock' or 'markov'")),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_model_canned_hello_response() {
        let m = MockModel;
        let toks = m.generate("hello world", None).unwrap();
        let joined = toks.join(" ");
        assert!(joined.starts_with("Hello!"));
        assert!(!toks.is_empty());
    }

    #[test]
    fn mock_model_echoes_non_hello() {
        let m = MockModel;
        let toks = m.generate("explain phase 3", None).unwrap();
        let joined = toks.join(" ");
        assert!(joined.contains("Mock generator received"));
    }

    #[test]
    fn markov_model_deterministic_for_same_prompt_and_seed() {
        let m = MarkovModel::default();
        let a = m.generate("the kernel", None).unwrap();
        let b = m.generate("the kernel", None).unwrap();
        assert_eq!(a, b, "same prompt + seed must yield same tokens");
    }

    #[test]
    fn markov_model_different_for_different_seeds() {
        let m = MarkovModel::default();
        let a = m.generate("the kernel", Some(1)).unwrap();
        let b = m.generate("the kernel", Some(999)).unwrap();
        // High probability of difference; very rare seed collisions
        // are possible but extremely unlikely for bigram over the
        // built-in corpus.
        assert_ne!(a, b, "different seeds must usually yield different tokens");
    }

    #[test]
    fn markov_model_returns_non_empty_output() {
        let m = MarkovModel::default();
        let toks = m.generate("the kernel", None).unwrap();
        assert!(!toks.is_empty(), "markov should produce at least the seed");
        // Output should not just echo the prompt verbatim.
        let prompt = "the kernel";
        let joined = toks.join(" ");
        assert_ne!(joined, prompt, "markov output must extend beyond the prompt");
    }

    #[test]
    fn markov_model_ngram_table_populated() {
        let m = MarkovModel::default();
        assert!(m.ngram_count() > 0, "default corpus should produce a non-empty table");
    }

    #[test]
    fn model_kind_parses_known_strings() {
        assert_eq!("mock".parse::<ModelKind>().unwrap(), ModelKind::Mock);
        assert_eq!("markov".parse::<ModelKind>().unwrap(), ModelKind::Markov);
        assert_eq!("MOCK".parse::<ModelKind>().unwrap(), ModelKind::Mock);
        assert!("unknown".parse::<ModelKind>().is_err());
    }

    #[test]
    fn model_kind_build_returns_a_model() {
        // Smoke: build() must produce something that responds
        // to generate().
        let m = ModelKind::Mock.build();
        let toks = m.generate("hi", None).unwrap();
        assert!(!toks.is_empty());

        let m = ModelKind::Markov.build();
        let toks = m.generate("hi", None).unwrap();
        assert!(!toks.is_empty());
    }
}

// ---------------------------------------------------------------------------
// Suppress unused-import warnings when only the ModelKind enum is in scope.
// ---------------------------------------------------------------------------

#[allow(dead_code)]
fn _value_helper() -> Option<Value> {
    None
}