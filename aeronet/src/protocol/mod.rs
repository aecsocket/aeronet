//! Parts of a generic aeronet protocol which can be used by implementors.
//!
//! Since different underlying transports have different capabilities, but this
//! crate aims to provide a basic set of capabilities accessible to all users
//! regardless of their chosen transport, some features must be implemented on
//! top of an existing transport layer.
//!
//! # Capabilities
//!
//! The typical capabilities that a transport provides are:
//! * **authentication** - ensures that the client and the server are talking to
//!   each other, rather than to some unknown third party. This could be done
//!   via e.g. a challenge token and a unique salt for each client to XOR
//!   encrypt packet data with.
//!   * Not provided by aeronet
//! * **encryption** - ensures that data between the endpoints cannot be read by
//!   a third party.
//!   * Not provided by aeronet
//! * **replay protection** - an attacker cannot break the protocol by recording
//!   sent packets, which are valid, and sending them back later.
//!   * Not provided by aeronet
//! * **validation** - ensuring that the other endpoint is communicating on the
//!   same version of the protocol as our endpoint.
//!   * Provided; TODO
//! * **fragmentation** - data larger than the MTU is split up into individual
//!   chunks by the sender, and reassembled on the other side (see
//!   [`MAX_PACKET_SIZE`]).
//!   * Provided; TODO
//! * **reliability** - the sender can identify when data was not received the
//!   other side, and resend data until it has been received.
//!   * Provided; TODO
//! * **ordering** - the receiver will receive messages in the order that they
//!   were sent, regardless of if the packets arrive out of order.
//!   * Provided; TODO
//! * **timeout/keep-alive** - the endpoint will disconnect itself if it has not
//!   received any data from the other side in some time, indicating a network
//!   failure; to ensure this does not happen in normal use, the endpoint can
//!   send a keep-alive message to get the other endpoint to respond back.
//!   * Provided; TODO
//!
//! Each capability is standalone - the transport can mix and match which
//! capabilities it wants to use from aeronet, and which ones it wants to use
//! from its underlying transport.
//!
//! # Lanes
//!
//! The aeronet protocol offers implementations of all the [`LaneKind`]s,
//! meeting all of their guarantees.
//!
//! The protocol is inspired by
//! <https://gafferongames.com/categories/building-a-game-network-protocol/>.

mod condition;
mod packet;
mod timeout;

pub use {condition::*, packet::*, timeout::*};
