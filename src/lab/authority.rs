//! Lab 01 — Authority.
//!
//! Question to answer: **a `Capability` is really authority, not a
//! token**.
//!
//! Experiment:
//! 1. Mint a Counter with `READ | WRITE | EXECUTE | ADMIN` (root).
//! 2. Verify all four operations succeed.
//! 3. `restrict` to `READ | WRITE` (drops EXECUTE + ADMIN).
//! 4. Verify `read` and `increment` succeed on the child; `reset`
//!    (which needs ADMIN) is denied.
//! 5. Bonus: try to `restrict` to `READ | ADMIN` (drops WRITE) and
//!    then back to `READ | WRITE | ADMIN` to demonstrate the kernel
//!    rejects any operation bit not present in the source.

use crate::capability::{CapabilityRights, OperationRights};
use crate::lab::{assert_authority, boot_counter, counter_cap, counter_slot, rights_summary};

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n== Lab 01 :: Authority ==\n");
    let h = boot_counter();
    let root = counter_cap(&h);
    let root_slot = counter_slot(&h);
    println!("  minted:    slot={} cap={}", root_slot.id(), root.id());
    println!("  root rights: {}", rights_summary(&root));

    println!("\n  --- root capabilities ---");
    assert_authority("read",      &root, OperationRights::READ,    serde_json::json!({"op": "read"}),      true);
    assert_authority("increment", &root, OperationRights::WRITE,   serde_json::json!({"op": "increment"}), true);
    assert_authority("reset",     &root, OperationRights::ADMIN,   serde_json::json!({"op": "reset"}),     true);

    println!("\n  --- restrict to READ | WRITE ---");
    let child_rights = CapabilityRights {
        operations: OperationRights::READ | OperationRights::WRITE,
        timeout_ms: root.rights().timeout_ms,
    };
    let child_id = h.cspace.restrict::<crate::plugins::counter::CounterResource>(
        root_slot.id(),
        child_rights,
        "counter_child".into(),
    )?;
    let child_slot = crate::capability::Slot::<crate::plugins::counter::CounterResource>::new(
        h.cspace.clone(),
        child_id,
    );
    let child_cap = child_slot
        .capability()
        .ok_or("child slot must be populated")?;
    println!("  child slot={child_id} rights={}", rights_summary(&child_cap));

    assert_authority("child read",      &child_cap, OperationRights::READ,    serde_json::json!({"op": "read"}),      true);
    assert_authority("child increment", &child_cap, OperationRights::WRITE,   serde_json::json!({"op": "increment"}), true);
    assert_authority("child reset",     &child_cap, OperationRights::ADMIN,   serde_json::json!({"op": "reset"}),     false);

    println!("\n  --- kernel refuses to grant authority not held ---");
    let too_many = CapabilityRights {
        operations: OperationRights::READ
            | OperationRights::WRITE
            | OperationRights::EXECUTE
            | OperationRights::ADMIN,
        timeout_ms: child_rights.timeout_ms,
    };
    match h.cspace.restrict::<crate::plugins::counter::CounterResource>(
        child_id,
        too_many,
        "counter_amplified".into(),
    ) {
        Ok(_) => println!("  ✗ kernel allowed authority amplification (BUG)"),
        Err(e) => println!("  ✓ kernel rejected: {e}"),
    }

    println!("\n  done.");
    Ok(())
}