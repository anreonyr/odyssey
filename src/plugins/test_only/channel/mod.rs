//! Channel plugin — Phase 2 P4: Capability-controlled communication.
//!
//! The plugin provides two resource types that work as a pair:
//!
//! - `ChannelResource` (the producer side): a `Resource` whose
//!   `invoke` forwards a JSON message over an internal `mpsc::Sender`.
//!   Holding `Capability<ChannelResource>` is the producer's
//!   authority to send. EXECUTE-gated.
//! - `ConsumerResource` (the receiver side): a `Resource` whose
//!   `invoke` blocks on the paired `mpsc::Receiver` and returns the
//!   next message as JSON. Holding `Capability<ConsumerResource>` is
//!   the consumer's authority to read. READ-gated.
//!
//! The Channel does not introduce a new authority system — both ends
//! are ordinary `Resource` types, so every existing capability
//! operation (`restrict`, `revoke`, `grant`, `transfer`, `restrict`)
//! applies identically. Revoking the producer's slot severs the
//! send-side lookup; revoking the consumer's slot severs the receive
//! side.
//!
//! ```text
//! Producer  --Capability<Channel>-->    Sender -> mpsc -> Receiver
//! Consumer  --Capability<Consumer>-->                            |
//!                                                              v
//!                                                          next message
//! ```

mod handler;
pub use handler::*;