# Odyssey Plugin Authority Graph

## 1. The Model

A Plugin in odyssey is a typed actor — a Rust `struct`
that implements `Resource` and runs inside an
orchestrator. Plugins exchange authority through
**Capabilities**: typed handles whose rights and quota
are enforced by the kernel.

The **Plugin Authority Graph** is the directed graph
induced by the live capabilities in the system:

- **Vertices**: `(PluginId, SlotId)` pairs. Each slot
  in each plugin's cspace is a vertex.
- **Edges**: derivations. An edge `parent → child` is
  inserted by `Slot::grant`, `Slot::transfer`,
  `cspace.grant_to`, `cspace.transfer_to`, or
  `Slot::restrict`. The edge carries a `CapabilityRights`
  payload.
- **Edge labels**: the three role bits `INVOKE`,
  `ASSIGN`, `REVOKE`. The label is `child ⊆ parent`
  — kernel-enforced.

A plugin that holds a slot can `invoke` it (call the
underlying `Resource::invoke`), `grant` or `restrict` it
(derive a child slot in its own cspace), `transfer` it
(move the slot to a fresh id, clearing the source),
or `revoke` it (clear the slot, with `revoke_tree`
cascading to descendants within the same cspace).

## 2. The Three Bits

### INVOKE

`Rights::INVOKE` authorises traversal: `slot.invoke(op, input)`
on a slot that holds `INVOKE` succeeds at the kernel
layer (`typed.rs:244`). Without `INVOKE`, the kernel
returns `CapabilityError::OperationDenied`. INVOKE is the
**only bit the kernel enforces on the call**.

### ASSIGN

`Rights::ASSIGN` authorises propagation: `slot.grant(...)`
and `cspace.grant_to(...)` succeed if the source slot's
held rights **attenuate** to the requested child rights
(bitmask subset — `CapabilityRights::contains`). The
kernel does **not** check that the caller holds `ASSIGN`
in its own rights; the only guard is the attenuation.
Phase 7 will add caller-identity.

### REVOKE

`Rights::REVOKE` authorises retraction: `cspace.revoke(...)`
and `cspace.revoke_tree(...)` succeed at the kernel layer
**regardless of the caller's rights**. There is no
caller-identity parameter; any holder of a `SlotId` can
clear any slot. Within a single cspace, `revoke_tree`
walks the `parents` map and clears every descendant.

## 3. Enforcement Surface

| Surface | File:Line | Enforced? |
| --- | --- | --- |
| `Capability<R>::invoke` | `crate/odyssey/src/capability/handle/cap/typed.rs:244` | Yes — INVOKE-only |
| `derive_with` (grant/transfer/restrict) | `crate/odyssey/src/capability/enforce/space.rs:780` | Yes — child ⊆ parent |
| `cspace.grant_to` | `crate/odyssey/src/capability/enforce/space.rs:403` | Yes — child ⊆ parent |
| `cspace.revoke` | `crate/odyssey/src/capability/enforce/space.rs:810` | No caller-identity |
| `cspace.revoke_tree` | `crate/odyssey/src/capability/enforce/space.rs:868` | No caller-identity |

## 4. Known Gaps (Phase 7 Hardening)

Three kernel facts are not enforced today:

1. **Caller-identity absent.** `grant_to`/`revoke` take
   only `SlotId` arguments; no `PluginId` is threaded
   through. The design doc Decision 3 ("REVOKE scoped to
   edges the holder created") is therefore a *latent*
   property, not an *enforced* one. Pin:
   `plugin_authority_misuse::revoke_without_revoke_bit_is_currently_allowed_known_gap`.

2. **`grant_to` does not populate `parents`.**
   `cspace.grant_to` calls `target.install` (space.rs:296)
   rather than `target.install_derived` (space.rs:549).
   Cross-cspace `revoke_tree` therefore does not reach
   across cspace boundaries. Pin:
   `plugin_authority_graph::revoke_tree_kills_grandchildren_in_same_cspace`
   asserts the surviving cross-cspace slot.

3. **ASSIGN caller check absent.** `slot.grant` and
   `cspace.grant_to` only check attenuation. A holder of
   `INVOKE`-only can still call `grant`; the attenuation
   check (`child ⊆ parent`) limits what they can produce,
   not whether they can produce it. Pin:
   `plugin_authority_graph::assign_delegation_a_to_c_can_invoke_and_regrant`
   asserts the attenuation behaviour, and the malicious
   tests document the absence of a stricter caller check.

## 5. Integration Points

A plugin integrates with the Authority Graph through
`CapabilityFactory`:

- `factory.plugin_cspace(plugin_id)` returns the
  plugin's `PluginCspace` (mint.rs:143). The plugin
  mints its capabilities into its own cspace via
  `pc.mint(...)` (space.rs:697).
- `pc.inner().grant_to::<R>(local_slot, target_pc.inner(), rights, name)`
  derives a slot into another plugin's cspace. The
  derived slot is owned by the target; the source is
  preserved.
- `pc.inner().transfer_to::<R>(local_slot, target_pc.inner(), name)`
  moves the slot to the target cspace, clearing the
  source.
- `pc.inner().revoke_tree(slot)` walks the local
  `parents` map and clears descendants.

The orchestrator's `default_ruin` (run.rs:91) revokes
global slot ids at teardown; `factory.reclaim_plugin`
(mint.rs:196) drains per-plugin cspaces.

## 6. Out of Scope

- **Performance.** The kernel's lock acquisition
  pattern (`parents → slots → names`) is correctness-
  oriented, not throughput-oriented. Phase 7 may add
  sharded allocation or lock-free fast paths.
- **CNode-equivalent badge hierarchies.** seL4 has
  badge bits alongside capabilities. odyssey does not.
- **Cross-process capabilities.** All Phase 6 tests
  run in-process; cross-process reachability is out of
  scope until the WASM loader lands.
- **Capability persistence.** Capabilities do not
  survive process restart. The session-checkpoint
  feature (`smoke.rs:2851`) handles session state,
  not authority.
