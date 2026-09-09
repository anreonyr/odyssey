//! Lab — Namespace (Phase 2 P6).
//!
//! Demonstrates that capabilities filed under hierarchical namespaces
//! are enumerable as a tree:
//!
//! ```text
//! odyssey
//! ├── odyssey.model
//! │   ├── odyssey.model.llama3.generate
//! │   └── odyssey.model.mock.generate
//! ├── odyssey.db
//! │   └── odyssey.db.read.query
//! └── odyssey.counter
//!     └── odyssey.counter.read
//! ```

use crate::capability::{graph::CapabilityGraph, CapabilityBudget, QuotaSpec};
use crate::host::factory::CapabilityFactory;
use crate::plugins::counter::CounterResource;
use crate::plugins::echo::EchoResource;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n== Lab :: Namespace ==\n");
    let cspace = crate::capability::CapabilitySpace::new();
    let factory = CapabilityFactory::new(cspace.clone());

    // Mint a few caps under hierarchical namespaces by setting the
    // plugin identity's `name` to the namespace root.
    let pid_llama = crate::host::manifest::PluginId {
        name: "odyssey.model.llama3".into(),
        version: "0.1.0".into(),
    };
    let pid_mock = crate::host::manifest::PluginId {
        name: "odyssey.model.mock".into(),
        version: "0.1.0".into(),
    };
    let pid_db = crate::host::manifest::PluginId {
        name: "odyssey.db.read".into(),
        version: "0.1.0".into(),
    };
    let pid_counter = crate::host::manifest::PluginId {
        name: "odyssey.counter".into(),
        version: "0.1.0".into(),
    };

    let make_decl = |cap_name: &str, streaming: bool| crate::host::manifest::CapabilityDecl {
        name: cap_name.into(),
        in_type: "object".into(),
        out_type: "object".into(),
        streaming,
        ..Default::default()
    };

    let budget = || CapabilityBudget::with_spec(5000, QuotaSpec::unlimited());

    factory.mint::<EchoResource>(
        crate::capability::CapKind::Sync,
        &make_decl("generate", false),
        &pid_llama,
        budget(),
        crate::plugins::echo::handler(),
    );
    factory.mint::<EchoResource>(
        crate::capability::CapKind::Sync,
        &make_decl("generate", false),
        &pid_mock,
        budget(),
        crate::plugins::echo::handler(),
    );
    factory.mint::<EchoResource>(
        crate::capability::CapKind::Sync,
        &make_decl("query", false),
        &pid_db,
        budget(),
        crate::plugins::echo::handler(),
    );
    factory.mint::<CounterResource>(
        crate::capability::CapKind::Sync,
        &make_decl("read", false),
        &pid_counter,
        budget(),
        crate::plugins::counter::handler(),
    );

    let graph = CapabilityGraph::from(&cspace);
    println!("{}", graph.render());

    println!("\n  --- children of \"odyssey\" ---");
    for (child, n) in cspace.namespace_children("odyssey") {
        println!("    {child}  ({n} caps)");
    }
    println!("\n  --- children of \"odyssey.model\" ---");
    for (child, n) in cspace.namespace_children("odyssey.model") {
        println!("    {child}  ({n} caps)");
    }

    println!("\n  done.");
    Ok(())
}