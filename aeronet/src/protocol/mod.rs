//! Provides implementations for parts of the aeronet protocol.
//!
//! Since not all transports will offer the same guarantees and use the same
//! protocol, this crate offers its own implementation of specific network
//! features which are agnostic to the underlying protocol (they just take in
//! and spit out bytes).
//!
//! # Terminology
//!
//! * *message* - the smallest unit of transmission used by the standard
//!   transport API, i.e. [`ClientTransport`] and [`ServerTransport`], but
//!   represented in its byte form ([`TryAsBytes`] / [`TryFromBytes`]).
//! * *packet* - the smallest unit of transmission used by the protocol, which
//!   holds a packet header and a payload.
//! * *packet header* - prefix to a packet which holds metadata about what it's
//!   carrying
//! * *payload* - either a part of, or the entirety of, the message that this
//!   packet wants to transport
//!
//! # Features
//!
//! Which features are provided by this protocol, and which must be implemented
//! externally?
//!
//! | Feature        | Description                                                                                        | Provided          |
//! |----------------|----------------------------------------------------------------------------------------------------|-------------------|
//! | encryption     | unauthorized third parties can't read the network data in transit                                  |                   |
//! | authentication | only clients who have permission to use this app can connect                                       |                   |
//! | validation     | the message was not tampered with or corrupted in transit                                          |                   |
//! | correct size   | bytes are received in the same "chunks" that they are sent - i.e. datagram style, not stream style |                   |
//! | fragmentation  | large messages are sent using multiple packets                                                     | [`Fragmentation`] |
//! | reliability    | messages will be resent until it's guaranteed that the receiver received the entire message        | todo              |
//! | ordering       | messages will be received in the same order they were sent                                         | todo              |
//!
//! # Fuzzing
//!
//! To ensure that protocol code works correctly in all situations, the code
//! makes use of both testing and fuzzing.
//!
//! To fuzz a particular component, run this from the `/aeronet` directory:
//! * [`Fragmentation`] - `cargo fuzz run frag`
//!
//! [`ClientTransport`]: crate::ClientTransport
//! [`ServerTransport`]: crate::ServerTransport
//! [`TryAsBytes`]: crate::TryAsBytes
//! [`TryFromBytes`]: crate::TryFromBytes

mod ack;
mod frag;
mod seq;
mod versioning;

pub use {ack::*, frag::*, seq::*, versioning::*};
