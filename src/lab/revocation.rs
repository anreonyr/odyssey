//! Lab 03 — Revocation.
//!
//! Question to answer: **revoking a slot makes lookups fail, even if
//! the holder still has the `Slot<R>` reference**.
//!
//! Experiment:
//! 1. Mint root Counter.
//! 2. `restrict` a child slot with READ | WRITE.
//! 3. Verify child works.
//! 4. `cspace.revoke(child_id)` — slot is gone.
//! 5. The child slot's `capability()` returns `None`; `invoke()`
//!    returns the typed error.
//! 6. Bonus: the *root* slot still works — revocation is local.

use crate::capability::{OperationRights, Slot};
use crate::lab::{assert_authority, boot_counter, counter_cap, rights_summary};
use crate::plugins::counter::CounterResource;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n== Lab 03 :: Revocation ==\n");
    let h = boot_counter();
    let root_cap = counter_cap(&h);
    let root_slot_id = h.counter_slot;
    let root_slot = Slot::<CounterResource>::new(h.cspace.clone(), root_slot_id);

    // Restrict to a child.
    let child_rights = crate::capability::CapabilityRights {
        operations: OperationRights::READ | OperationRights::WRITE,
        timeout_ms: 5000,
    };
    let child_id = h.cspace.restrict::<CounterResource>(
        root_slot_id,
        child_rights,
        "counter_child".into(),
    )?;
    let child_slot = Slot::<CounterResource>::new(h.cspace.clone(), child_id);
    let child_cap = child_slot.capability().ok_or("child slot empty")?;
    println!("  root  slot={root_slot_id} rights={}", rights_summary(&root_cap));
    println!("  child slot={child_id} rights={}", rights_summary(&child_cap));

    println!("\n  --- before revoke ---");
    assert_authority(
        "child read",
        &child_cap,
        OperationRights::READ,
        serde_json::json!({"op": "read"}),
        true,
    );

    println!("\n  --- cspace.revoke(child) ---");
    let removed = h.cspace.revoke(child_id);
    println!("  revoke({child_id}) → {removed}");

    // The slot reference is still valid (it doesn't panic), but the
    // capability it points at is gone.
    let after = child_slot.capability();
    println!(
        "  child_slot.capability() after revoke = {}",
        if after.is_some() {
            "Some(...)"
        } else {
            "None"
        }
    );
    match child_slot.invoke(serde_json::json!({"op": "read"})) {
        Ok(v) => println!("  ✗ unexpected success: {v}"),
        Err(e) => println!("  ✓ child invoke denied: {e}"),
    }
    // Also check the slot directly via Slot::invoke (uses capability()).
    match child_slot.invoke(serde_json::json!({"op": "read"})) {
        Ok(v) => println!("  ✗ unexpected second success: {v}"),
        Err(e) => println!("  ✓ second call denied: {e}"),
    }

    // Root is unaffected.
    println!("\n  --- root is untouched ---");
    assert_authority(
        "root read",
        &root_cap,
        OperationRights::READ,
        serde_json::json!({"op": "read"}),
        true,
    );

    // Re-mint and verify revoke idempotency.
    println!("\n  --- revoke is idempotent ---");
    let removed_again = h.cspace.revoke(child_id);
    println!("  revoke({child_id}) again → {removed_again} (expected false)");

    println!("\n  done.");
    let _ = root_slot; // keep warning-free
    Ok(())
}