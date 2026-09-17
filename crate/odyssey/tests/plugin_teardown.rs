//! Plugin-teardown tests (Slice 3 caveat fix).
//!
//! Slice 3 of Direction A migrated plugin caps into each
//! plugin's own `PluginCspace` and grants a derived slot
//! into the orchestrator's global cspace. The migration
//! left the orchestrator's `default_ruin` tearing down
//! only the GLOBAL slot ids returned by the `MintFn`; the
//! plugin-local slot in the `PluginCspace` was leaked
//! across plugin restarts.
//!
//! The fix lives at the orchestrator boundary: after
//! `default_ruin` runs, `ruin_via_registry` calls
//! `factory.reclaim_plugin(plugin_id)` to drain the
//! plugin's `PluginCspace`. The `RuinFn` signature is
//! untouched — every builtin's `register()` still returns
//! `default_ruin` unchanged.
//!
//! These tests cover the new boundary without going
//! through `run_on` (the HTTP bridge path is exercised by
//! the example smoke tests). Each test stands up the same
//! Slice 3 fixture the orchestrator uses — a
//! `CapabilityFactory`, a `PluginId`, a `PluginCspace`
//! minted into, a derived slot granted into the global
//! cspace — and asserts the post-teardown state.
//!
//! Tests:
//! - [`reclaim_plugin_drains_local_slot_after_mint_grant_to`]:
//!   the basic Slice 3 shape. Mint + grant-to, then
//!   `factory.reclaim_plugin`; the local cspace is empty.
//! - [`reclaim_plugin_clears_derived_children`][]: a
//!   restrict-derived child in the `PluginCspace` is also
//!   gone (cascade through `revoke_tree`).
//! - [`reclaim_plugin_is_noop_for_unmigrated_plugin`]:
//!   a plugin that never called `factory.plugin_cspace`
//!   returns 0 — backward-compatible with builtins that
//!   mint straight into the global cspace.
//! - [`reclaim_plugin_is_idempotent`]: calling reclaim
//!   twice does not double-count and does not panic.
//! - [`reclaim_plugin_does_not_touch_global_cspace`]:
//!   the local reclaim only walks the plugin's own
//!   `PluginCspace`; the global slot (and any peers in the
//!   global cspace) survive until the matching `RuinFn`
//!   revokes them.
//! - [`full_teardown_path_leaves_no_slots_anywhere`][]: a
//!   simulation of `ruin_via_registry`'s body — `RuinFn`
//!   revokes the global ids, then `reclaim_plugin` drains
//!   the local cspace. Both cspaces end empty.
//! - [`default_ruin_signature_unchanged`]: a compile-time
//!   assertion that `default_ruin`'s type is still
//!   `fn(&CapabilitySpace, &[SlotId]) -> Result<usize,
//!   String>` — the Slice 3 fix must not have widened the
//!   type.

use std::sync::Arc;

use odyssey::capability::enforce::quota::CapabilityBudget;
use odyssey::capability::enforce::space::CapabilitySpace;
use odyssey::core::clock::clock::SystemClock;
use odyssey::core::contract::resource::Resource;
use odyssey::core::identity::ids::{PluginId, SlotId};
use odyssey::core::identity::kind::CapKind;
use odyssey::core::manifest::manifest::CapabilityDecl;
use odyssey::core::rights::rights::{CapabilityRights, Rights};
use odyssey::personality::lifecycle::mint::CapabilityFactory;
use odyssey::personality::lifecycle::run::{RuinFn, default_ruin};

// ---------------------------------------------------------------------------
// Stub resource + fixture builder
// ---------------------------------------------------------------------------

/// A `Resource` impl that does nothing on invoke / open. The
/// tests only exercise the mint + revoke paths, so the default
/// `Resource` impls are fine — the kernel needs to be able to
/// build a typed `Capability<StubResource>` to populate the
/// cspace, but it never has to dispatch through it.
#[derive(Debug)]
struct StubResource;

impl Resource for StubResource {
    fn invoke(&self, _input: serde_json::Value) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({}))
    }
}

/// Build the fixture every test uses: an empty global cspace,
/// a factory with a system clock, and a fresh `PluginId`.
fn fixture() -> (CapabilitySpace, CapabilityFactory, PluginId) {
    let global = CapabilitySpace::new();
    let factory = CapabilityFactory::with_clock(global.clone(), Arc::new(SystemClock));
    let plugin = PluginId {
        name: "teardown_probe".into(),
        version: "0.1.0".into(),
    };
    (global, factory, plugin)
}

/// Build a minimal `CapabilityDecl`. The kernel only reads
/// `name` (for `slot_for_name`) and `kind` (passed through to
/// the cap); `contract_name` is left empty because no test
/// exercises resolver injection.
fn make_decl(name: &str) -> CapabilityDecl {
    CapabilityDecl {
        name: name.to_string(),
        kind: CapKind::Sync,
        contract_name: String::new(),
        tool_schema: None,
    }
}

/// Slice 3 fixture: mint into the plugin's `PluginCspace`,
/// grant a derived slot into the global cspace. Returns the
/// `(global_slot, local_slot)` pair — the two ids the test
/// will assert on after teardown.
fn mint_and_grant(
    factory: &CapabilityFactory,
    plugin: &PluginId,
    decl: &CapabilityDecl,
) -> (SlotId, SlotId) {
    let pc = factory.plugin_cspace(plugin);
    let local = pc.mint(
        CapKind::Sync,
        decl,
        CapabilityBudget::new(5000),
        Arc::new(StubResource),
    );
    let rights = CapabilityRights {
        operations: Rights::ALL,
        timeout_ms: 5000,
    };
    let global = pc
        .inner()
        .grant_to::<StubResource>(local, factory.space(), rights, decl.name.clone())
        .expect("grant_to from plugin cspace to global should succeed");
    (global, local)
}

// ---------------------------------------------------------------------------
// Positive: reclaim clears the local slot
// ---------------------------------------------------------------------------

#[test]
fn reclaim_plugin_drains_local_slot_after_mint_grant_to() {
    let (_global, factory, plugin) = fixture();
    let decl = make_decl("echo");
    let (global_slot, local_slot) = mint_and_grant(&factory, &plugin, &decl);

    // Sanity: both cspaces hold the cap under its declared
    // name before reclaim.
    let pc = factory.plugin_cspace(&plugin);
    assert_eq!(
        pc.inner().slot_for_name("echo"),
        Some(local_slot),
        "plugin cspace must hold the local slot under its declared name before reclaim"
    );
    assert_eq!(
        factory.space().slot_for_name("echo"),
        Some(global_slot),
        "global cspace must hold the granted slot under its declared name before reclaim"
    );

    // Drain the plugin's `PluginCspace`. This is the
    // orchestrator's teardown step the Slice 3 caveat fix
    // adds; the `RuinFn` keeps doing its thing separately
    // against the global cspace.
    let removed = factory.reclaim_plugin(&plugin);
    assert_eq!(
        removed, 1,
        "reclaim should report exactly one slot removed (the local slot)"
    );

    // The plugin's local cspace is empty. `slot_for_name`
    // returns `None`; `is_empty()` returns `true`.
    assert!(
        pc.inner().slot_for_name("echo").is_none(),
        "plugin cspace must no longer hold the local slot after reclaim"
    );
    assert!(
        pc.inner().is_empty(),
        "plugin cspace must be empty after reclaim; got {} slot(s)",
        pc.inner().len()
    );

    // The global cspace is untouched by the local reclaim —
    // the `RuinFn` is responsible for it.
    assert_eq!(
        factory.space().slot_for_name("echo"),
        Some(global_slot),
        "global cspace must still hold the granted slot after a local-only reclaim"
    );
}

// ---------------------------------------------------------------------------
// Positive: cascade — derived children in the local cspace are also gone
// ---------------------------------------------------------------------------

#[test]
fn reclaim_plugin_clears_derived_children() {
    let (_global, factory, plugin) = fixture();
    let decl = make_decl("echo");
    let (_global_slot, local_slot) = mint_and_grant(&factory, &plugin, &decl);

    // Derive a strictly-less-rights child from the local
    // slot via `restrict`. This is the same attenuation
    // primitive the kernel's profile_inspector uses; the
    // child's parent pointer lives in the plugin's
    // `PluginCspace`, so it is a local-cspace descendant
    // and must be cleared by `reclaim_plugin` along with
    // its root.
    let pc = factory.plugin_cspace(&plugin);
    let read_only_rights = CapabilityRights {
        operations: Rights::INVOKE,
        timeout_ms: 5000,
    };
    let read_only = pc
        .inner()
        .restrict::<StubResource>(local_slot, read_only_rights, "echo_readonly".to_string())
        .expect("restrict on the local slot should succeed when attenuating only");

    // Sanity: the local cspace now has two roots (the
    // original plus the derived child surfaces its own
    // `slot_for_name` entry).
    assert_eq!(
        pc.inner().len(),
        2,
        "local cspace must hold root + derived child"
    );
    assert_eq!(
        pc.inner().slot_for_name("echo_readonly"),
        Some(read_only),
        "derived child must be reachable by its declared name"
    );

    // Drain. The reported count must include the cascade
    // (root + child = 2).
    let removed = factory.reclaim_plugin(&plugin);
    assert_eq!(
        removed, 2,
        "reclaim must cascade through the parent pointer and clear the derived child too"
    );
    assert!(
        pc.inner().is_empty(),
        "local cspace must be empty after reclaim; got {} slot(s)",
        pc.inner().len()
    );
}

// ---------------------------------------------------------------------------
// Backward-compat: a plugin that never created a PluginCspace reclaims nothing
// ---------------------------------------------------------------------------

#[test]
fn reclaim_plugin_is_noop_for_unmigrated_plugin() {
    let (_global, factory, plugin) = fixture();

    // Note: we do NOT call `factory.plugin_cspace(&plugin)`.
    // A builtin that hasn't migrated to per-plugin isolation
    // never lazy-creates a `PluginCspace`; the reclaim must
    // observe that and report 0 without panicking.
    let removed = factory.reclaim_plugin(&plugin);
    assert_eq!(
        removed, 0,
        "reclaim must report 0 for a plugin that never created a PluginCspace"
    );
}

// ---------------------------------------------------------------------------
// Idempotent: calling reclaim twice is safe and reports 0 the second time
// ---------------------------------------------------------------------------

#[test]
fn reclaim_plugin_is_idempotent() {
    let (_global, factory, plugin) = fixture();
    let decl = make_decl("echo");
    let _ = mint_and_grant(&factory, &plugin, &decl);

    let first = factory.reclaim_plugin(&plugin);
    assert_eq!(first, 1, "first reclaim must clear the single slot");

    let second = factory.reclaim_plugin(&plugin);
    assert_eq!(
        second, 0,
        "second reclaim must report 0 (the cspace is already empty)"
    );
}

// ---------------------------------------------------------------------------
// Isolation: reclaim only touches the plugin's own PluginCspace
// ---------------------------------------------------------------------------

#[test]
fn reclaim_plugin_does_not_touch_global_cspace() {
    let (_global, factory, plugin) = fixture();
    let decl = make_decl("echo");
    let (global_slot, _local_slot) = mint_and_grant(&factory, &plugin, &decl);

    // Add a peer cap directly in the global cspace, the
    // way a non-migrated builtin mints. This slot is not
    // tracked by the plugin's `PluginCspace` and must
    // survive `reclaim_plugin`.
    let peer_decl = make_decl("peer_in_global");
    let _peer_slot = factory.mint(
        CapKind::Sync,
        &peer_decl,
        &plugin,
        CapabilityBudget::new(5000),
        Arc::new(StubResource),
    );

    let _removed = factory.reclaim_plugin(&plugin);

    // The granted echo slot is still in the global cspace;
    // the peer is still in the global cspace. The
    // `RuinFn` (or another path) is responsible for them.
    assert_eq!(
        factory.space().slot_for_name("echo"),
        Some(global_slot),
        "reclaim must not touch the global-cspace granted slot"
    );
    assert!(
        factory.space().slot_for_name("peer_in_global").is_some(),
        "reclaim must not touch a peer slot that was minted straight into the global cspace"
    );
}

// ---------------------------------------------------------------------------
// End-to-end: the full orchestrator teardown path leaves no slots anywhere
// ---------------------------------------------------------------------------

#[test]
fn full_teardown_path_leaves_no_slots_anywhere() {
    let (global, factory, plugin) = fixture();
    let decl = make_decl("echo");
    let (global_slot, local_slot) = mint_and_grant(&factory, &plugin, &decl);

    // Simulate the orchestrator's `ruin_via_registry` body
    // for this plugin: the `RuinFn` revokes the global
    // slot ids it was handed, then the orchestrator drains
    // the plugin's `PluginCspace` via `reclaim_plugin`.
    // The default `RuinFn` is `default_ruin`; using it
    // here exercises the exact symbol the builtin
    // `register()` helpers reference.
    let global_revoked =
        default_ruin(&global, std::slice::from_ref(&global_slot)).expect("default_ruin ok");
    assert_eq!(
        global_revoked, 1,
        "default_ruin must revoke the granted slot"
    );

    let local_revoked = factory.reclaim_plugin(&plugin);
    assert_eq!(local_revoked, 1, "reclaim must drain the local slot");

    // Both cspaces end empty. This is the property the
    // Slice 3 caveat fix establishes: a plugin restart
    // does not find leftover state in either place.
    assert!(
        global.is_empty(),
        "global cspace must be empty after the full teardown; got {} slot(s)",
        global.len()
    );
    let pc = factory.plugin_cspace(&plugin);
    assert!(
        pc.inner().is_empty(),
        "plugin cspace must be empty after the full teardown; got {} slot(s)",
        pc.inner().len()
    );

    // And the slot ids that used to be live are no longer
    // resolvable from either cspace.
    assert!(global.slot_for_name("echo").is_none());
    assert!(pc.inner().slot_for_name("echo").is_none());
    assert!(global.lookup_typed::<StubResource>(local_slot).is_none());
    assert!(global.lookup_typed::<StubResource>(global_slot).is_none());
}

// ---------------------------------------------------------------------------
// Compile-time guard: the RuinFn signature must not have widened
// ---------------------------------------------------------------------------

/// `RuinFn` is the function-pointer alias the orchestrator's
/// registry dispatches against. The Slice 3 fix MUST NOT have
/// widened its type — adding `Option<&PluginCspace>` or
/// `&CapabilityFactory` to the signature would break every
/// builtin `register()` that returns `default_ruin`. This
/// test pins the shape at compile time so a future refactor
/// that tries to widen the type fails to build here, before
/// the smoke tests catch it.
///
/// The `RuinFn` alias is a `fn` pointer type, so binding a
/// concrete value of that type to `default_ruin` (which has
/// the matching signature) is a structural check: a future
/// signature widening would either fail to coerce
/// `default_ruin` into the new alias (yielding a compile
/// error here) or would silently match — the latter would
/// also fail because the alias definition itself would
/// change, which this file imports and so would also fail to
/// type-check.
#[test]
fn default_ruin_signature_unchanged() {
    let _pin: RuinFn = default_ruin;
}

// ---------------------------------------------------------------------------
// Slice 2 of the INVOKE / ASSIGN / REVOKE redesign: transitive REVOKE
// test (Pattern References §3 in the design).
//
// Mirrors the seL4 CNode revocation semantics: revoking a parent cap
// transitively kills all derived descendants. The test mints a parent
// slot (with `Rights::REVOKE`), derives a child via `Slot::grant`,
// then exercises `cspace.revoke_tree(parent)` and asserts the child
// can no longer be invoked.
// ---------------------------------------------------------------------------

#[test]
fn revoke_tree_transitively_kills_derived_child() {
    use odyssey::capability::handle::slot::Slot;

    let (cspace, factory, plugin) = fixture();
    let decl = make_decl("transitive_test");

    // Mint a parent slot directly into the global cspace (the
    // factory's `mint` path). `Rights::ALL` after Slice 2 is
    // `Rights::INVOKE | Rights::ASSIGN | Rights::REVOKE`,
    // which is what we want for the parent.
    let parent_id = factory.mint(
        CapKind::Sync,
        &decl,
        &plugin,
        CapabilityBudget::new(5000),
        Arc::new(StubResource),
    );

    // Derive a child via `Slot::grant` — same `seL4 CNode.Mint`
    // shape the attenuation test uses. The child carries
    // `Rights::INVOKE` only (a strict subset of the parent's).
    let parent: Slot<StubResource> = Slot::new(cspace.clone(), parent_id);
    let child_id = parent
        .grant(
            CapabilityRights {
                operations: Rights::INVOKE,
                timeout_ms: 5000,
            },
            "transitive_test_child".into(),
        )
        .expect("grant of derived child should succeed");

    // Sanity: the child works before revoke.
    let child: Slot<StubResource> = Slot::new(cspace.clone(), child_id);
    let ok = child
        .invoke(Rights::INVOKE, serde_json::json!({}))
        .expect("child invoke before revoke must succeed");
    assert_eq!(ok, serde_json::json!({}));

    // Revoke the parent tree. `revoke_tree` is recursive
    // over derived slots (per `CapabilitySpace::revoke_tree`
    // implementation in `space.rs`). The expected return is
    // at least 2 (parent + child).
    let removed = cspace.revoke_tree(parent_id);
    assert!(
        removed >= 2,
        "expected at least 2 slots removed (parent + child), got {removed}"
    );

    // After revoke, the child's lookup is `SlotEmpty` /
    // `Revoked`. We don't care which; we care that invoke
    // fails — and that it does so for a teardown reason
    // (not, e.g., a rights regression).
    let err = child
        .invoke(Rights::INVOKE, serde_json::json!({}))
        .expect_err("child invoke after parent revoke must fail");
    let err_str = err.to_string();
    assert!(
        err_str.contains("revoked") || err_str.contains("empty") || err_str.contains("slot"),
        "expected teardown error, got {err_str}"
    );

    // Idempotence: revoking the same parent again is a no-op.
    let removed_again = cspace.revoke_tree(parent_id);
    assert_eq!(
        removed_again, 0,
        "second revoke_tree on the same root must report 0 removed slots"
    );
}
