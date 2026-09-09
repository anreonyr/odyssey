//! Echo-chain — compound `EchoChainResource: Resource` that wraps a
//! `Capability<EchoResource>` and returns a chained envelope on invoke.

mod handler;
pub use handler::*;
