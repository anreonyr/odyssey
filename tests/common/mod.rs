//! Shared helpers for the integration tests under `tests/`.
//!
//! Cargo treats anything directly in `tests/` as a test binary, but
//! `tests/common/` is just a module directory — its `mod.rs` is
//! compiled into each test crate that declares `mod common;`.
//! Helpers that aren't used by a particular crate are dead from
//! that crate's perspective, so every `pub fn` here carries
//! `#[allow(dead_code)]` to silence per-crate false positives.
//!
//! Every test boots its own CSpace + factory via [`boot`]. Tests that
//! want to share state across multiple mints reuse the same factory.
//!
//! The `mint_*` helpers all assume the conventional in/out types used
//! by the runtime plugins (`in_type = "object"`, `out_type = "object"`,
//! `streaming = false` for counters / brokers; `"any"` for echo /
//! slow). Tests that need to vary these build a `CapabilityDecl` by
//! hand instead.

use std::sync::{Arc, OnceLock};

use odyssey::capability::{
    Capability, CapabilityBudget, CapabilityRights, CapabilitySpace, CapKind, OperationRights,
    QuotaSpec, SlotId,
};
use odyssey::kernel::factory::CapabilityFactory;
use odyssey::kernel::manifest::{CapabilityDecl, PluginId, PluginManifest};
use odyssey::plugins::test_only::broker::{handler as broker_handler, BrokerResource};
use odyssey::plugins::test_only::counter::CounterResource;

/// Default timeout every test budget uses. Generous enough that
/// real-handler latency (a 200ms `slow` sleep) still finishes, but
/// not so loose that an obviously-wrong invocation takes forever to
/// fail.
pub const DEFAULT_TIMEOUT_MS: u32 = 5000;

/// Read the counter's manifest from disk, cached per-test-crate.
/// Used by [`mint_counter`] so every test gets the same contract
/// the runtime counter has — including the action → OperationRights
/// table that the `RuleAgent` consults. Each test crate has its
/// own copy of `common::mod` (cargo compiles the helper once per
/// test binary), so the cache lives for the lifetime of one test
/// binary's ~24 tests, not forever.
fn load_counter_manifest() -> &'static PluginManifest {
    static CACHE: OnceLock<PluginManifest> = OnceLock::new();
    CACHE.get_or_init(|| {
        let toml_src = std::fs::read_to_string("src/plugins/test_only/counter/counter.toml")
            .expect("counter.toml present at src/plugins/test_only/counter/counter.toml");
        PluginManifest::from_toml_str(&toml_src).expect("counter.toml parses")
    })
}

/// Fresh CSpace + Factory. Every test should boot its own — no shared
/// state across tests, so failures localise.
#[allow(dead_code)]
pub fn boot() -> (CapabilitySpace, CapabilityFactory) {
    let space = CapabilitySpace::new();
    let factory = CapabilityFactory::new(space.clone());
    (space, factory)
}

/// Conventional `CapabilityDecl` for a counter: object in, object out,
/// sync, with no contract. Tests that need a real contract build a
/// `CapabilityDecl` literal with `..Default::default()` and override
/// `contract`.
#[allow(dead_code)]
pub fn counter_decl(name: &str) -> CapabilityDecl {
    CapabilityDecl {
        name: name.into(),
        in_type: "object".into(),
        out_type: "object".into(),
        streaming: false,
        ..Default::default()
    }
}

/// Conventional `PluginId` for the counter plugin.
#[allow(dead_code)]
pub fn counter_pid() -> PluginId {
    PluginId {
        name: "counter".into(),
        version: "0.1.0".into(),
    }
}

/// "All authority, default timeout" rights bag — the root cap shape.
#[allow(dead_code)]
pub fn all_rights() -> CapabilityRights {
    CapabilityRights {
        operations: OperationRights::ALL,
        timeout_ms: DEFAULT_TIMEOUT_MS,
    }
}

/// Mint a `Capability<CounterResource>` with full authority into the
/// given factory. Reads the canonical counter manifest so the
/// minted cap carries the real contract (including the
/// `actions` table that the `RuleAgent` reads); this matches what
/// `cargo run -- boot` produces for the runtime counter.
#[allow(dead_code)]
pub fn mint_counter(factory: &CapabilityFactory) -> SlotId {
    let m = load_counter_manifest();
    factory.mint::<CounterResource>(
        CapKind::Sync,
        &m.exposes[0],
        &m.plugin,
        CapabilityBudget::new(DEFAULT_TIMEOUT_MS),
        odyssey::plugins::test_only::counter::handler(),
    )
}

/// Same as [`mint_counter`] but with a `QuotaSpec` attached so quota
/// tests can exercise the kernel-level rate limit.
#[allow(dead_code)]
pub fn mint_counter_with_quota(
    factory: &CapabilityFactory,
    name: &str,
    quota: QuotaSpec,
) -> SlotId {
    factory.mint::<CounterResource>(
        CapKind::Sync,
        &counter_decl(name),
        &counter_pid(),
        CapabilityBudget::with_spec(DEFAULT_TIMEOUT_MS, quota),
        odyssey::plugins::test_only::counter::handler(),
    )
}

/// Mint an Echo resource (true passthrough) at the named slot.
#[allow(dead_code)]
pub fn mint_echo(factory: &CapabilityFactory, name: &str) -> SlotId {
    factory.mint::<odyssey::plugins::echo::basic::EchoResource>(
        CapKind::Sync,
        &CapabilityDecl {
            name: name.into(),
            in_type: "any".into(),
            out_type: "any".into(),
            streaming: false,
            ..Default::default()
        },
        &PluginId {
            name: "echo".into(),
            version: "0.1.0".into(),
        },
        CapabilityBudget::new(DEFAULT_TIMEOUT_MS),
        odyssey::plugins::echo::basic::handler(),
    )
}

/// Mint a Slow resource (handler sleeps 200ms per call). The default
/// 5s timeout is generous enough for the sleep to finish.
#[allow(dead_code)]
pub fn mint_slow(factory: &CapabilityFactory) -> SlotId {
    factory.mint::<odyssey::plugins::slow::SlowResource>(
        CapKind::Sync,
        &CapabilityDecl {
            name: "slow".into(),
            in_type: "any".into(),
            out_type: "any".into(),
            streaming: false,
            ..Default::default()
        },
        &PluginId {
            name: "slow".into(),
            version: "0.1.0".into(),
        },
        CapabilityBudget::new(DEFAULT_TIMEOUT_MS),
        odyssey::plugins::slow::handler(),
    )
}

/// Mint a Broker that closes over the given counter capability and
/// parent slot id. Used by the delegation and multi-hop tests.
#[allow(dead_code)]
pub fn mint_broker(
    factory: &CapabilityFactory,
    counter: Arc<Capability<CounterResource>>,
    slot: SlotId,
    space: &CapabilitySpace,
) -> SlotId {
    factory.mint::<BrokerResource>(
        CapKind::Sync,
        &CapabilityDecl {
            name: "broker".into(),
            in_type: "object".into(),
            out_type: "object".into(),
            streaming: false,
            ..Default::default()
        },
        &PluginId {
            name: "broker".into(),
            version: "0.1.0".into(),
        },
        CapabilityBudget::new(DEFAULT_TIMEOUT_MS),
        broker_handler(counter, slot, space.clone()),
    )
}

/// "Empty contract" — kept for back-compat with older tests that
/// import this helper. Phase 3 P3.2 split the old
/// `CapabilityContract` into `AuthorityContract` (action → op)
/// and `Protocol` (wire metadata). This helper now returns an
/// empty `AuthorityContract`. Tests that want a `Protocol`
/// should construct one directly.
#[allow(dead_code)]
pub fn empty_contract() -> odyssey::capability::AuthorityContract {
    odyssey::capability::AuthorityContract::default()
}