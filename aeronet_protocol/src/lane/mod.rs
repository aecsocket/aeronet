//! Protocol-level implementation of lanes and associated features such as
//! message acknowledgements and ordering.
//!
//! The most important type to users here is [`Lanes`], which allows processing
//! incoming packets and building up outgoing packets. It handles all the lane
//! guarantees such as reliability and ordering.
//!
//! The API aims to minimize allocations - it will attempt to reuse the user's
//! allocations by using [`Bytes`]:
//! * as input parameters to the API
//! * internally, to store message payloads
//! * as output items for e.g. the outgoing packet iterator
//!
//! # Usage
//!
//! * API user creates [`Lanes`] with a user-defined config
//! * When the user wants to send a message, they call [`buffer_send`] to buffer
//!   up the message for sending
//!   * The message is sent by giving ownership of [`Bytes`], allowing the lane
//!     to reuse the user's allocation
//!   * This will not immediately send the message out, but will buffer it
//! * On app update, the user calls these functions in this specific order:
//!   * [`recv`] to forward transport packets to the lanes
//!   * [`poll`] to update the lanes' internal states
//!   * [`flush`] to forward the lanes' packets to the transport
//! * For each incoming packet from the lower-level transport (i.e. Steamworks,
//!   WebTransport, etc.), the packet is passed to [`recv`]
//!   * The lane processes this packet and returns an iterator over the messages
//!     that it contains - a single packet may contain between 0 or more
//!     messages
//! * [`poll`] is called to update the internal state
//!   * If this returns an error, the connection must be terminated
//! * [`flush`] returns an iterator over all packets to forward to the
//!   lower-level transport
//!   * All packets must be sent down the transport
//!
//! [`buffer_send`]: Lanes::buffer_send
//! [`recv`]: Lanes::recv
//! [`poll`]: Lanes::poll
//! [`flush`]: Lanes::flush
//!
//! # Encoded layout
//!
//! Types:
//! * [`Varint`](octets::varint_len) - a `u64` encoded using between 1 and 10
//!   bytes, depending on the value. Smaller values are encoded more
//!   efficiently.
//! * [`AcknowledgeHeader`](crate::AcknowledgeHeader)
//! * [`FragmentHeader`](crate::FragmentHeader)
//! * [`Seq`](crate::Seq)
//!
//! ```ignore
//! struct Packet {
//!     /// Which lane this packet is sent on, and meant to be received on.
//!     lane_index: Varint,
//!     /// Lane-specific payload. How this is deserialized depends on the
//!     /// `lane_index`.
//!     payload: Either<UnreliablePayload, ReliablePayload>,
//! }
//!
//! struct UnreliablePayload {
//!     /// All fragments carried by this packet.
//!     frags: [Fragment],
//! }
//!
//! struct ReliablePayload {
//!     /// Acknowledge response data marking which fragments the sender of
//!     /// *this* packet has received.
//!     ack_header: AcknowledgeHeader,
//!     /// All fragments carried by this packet.
//!     frags: [Fragment],
//! }
//!
//! struct Fragment {
//!    /// Metadata on what fragment this actually represents.
//!    frag_header: FragmentHeader
//!    /// Length of the upcoming payload.
//!    payload_len: Varint,
//!    /// User-defined message payload.
//!    payload: [u8],
//! }
//! ```

mod lanes;
mod ord;
mod packet;

pub mod reliable;
pub mod unreliable;

pub use {lanes::*, ord::*, packet::LanePacket, reliable::Reliable, unreliable::Unreliable};

use bytes::Bytes;
use enum_dispatch::enum_dispatch;

use crate::{FragmentError, ReassembleError, Seq};

#[derive(Debug, Clone, thiserror::Error)]
pub enum LaneError {
    #[error("failed to receive a message in time")]
    RecvTimeout,
    #[error("failed to send a message in time")]
    SendTimeout,

    #[error("failed to fragment message")]
    Fragment(#[source] FragmentError),

    #[error("input too short to contain lane index")]
    NoLaneIndex,
    #[error("invalid lane index {lane_index}")]
    InvalidLane { lane_index: usize },
    #[error("fragment reported {len} bytes, but buffer only had {cap} more")]
    TooLong { len: usize, cap: usize },
    #[error("input too short to contain sequence number")]
    NoSeq,
    #[error("input too short to contain ack header data")]
    NoAckHeader,
    #[error("input too short to contain frag header data")]
    NoFragHeader,
    #[error("frag header is invalid")]
    InvalidFragHeader,
    #[error("failed to reassemble payload")]
    Reassemble(#[source] ReassembleError),
}

#[derive(Debug)]
pub enum LaneFlush<'l> {
    Unreliable(unreliable::Flush<'l>),
    Reliable(reliable::Flush<'l>),
}

impl Iterator for LaneFlush<'_> {
    type Item = LanePacket;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Unreliable(iter) => iter.next(),
            Self::Reliable(iter) => iter.next(),
        }
    }
}

pub enum LaneRecv<'l> {
    UnreliableUnordered(unreliable::Recv<'l, Unordered>),
    UnreliableSequenced(unreliable::Recv<'l, Sequenced>),
    ReliableUnordered(reliable::Recv<'l, Unordered>),
    ReliableSequenced(reliable::Recv<'l, Sequenced>),
    ReliableOrdered(reliable::Recv<'l, Ordered>),
}

impl Iterator for LaneRecv<'_> {
    type Item = Result<Bytes, LaneError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::UnreliableUnordered(iter) => iter.next(),
            Self::UnreliableSequenced(iter) => iter.next(),
            Self::ReliableUnordered(iter) => iter.next(),
            Self::ReliableSequenced(iter) => iter.next(),
            Self::ReliableOrdered(iter) => iter.next(),
        }
    }
}

// impl

const VARINT_MAX_SIZE: usize = 10;

// impl details: I considered adding a `message_state` here to get the current
// state of a message by its Seq, but imo this is too unreliable and we can't
// provide strong enough guarantees to get the *current* state of a message
#[enum_dispatch]
trait LaneState {
    fn buffer_send(&mut self, msg: Bytes) -> Result<Seq, LaneError>;

    fn recv(&mut self, packet: Bytes) -> Result<LaneRecv<'_>, LaneError>;

    fn poll(&mut self) -> Result<(), LaneError>;

    fn flush(&mut self) -> LaneFlush<'_>;
}

#[derive(Debug)]
#[enum_dispatch(LaneState)]
enum Lane {
    UnreliableUnordered(Unreliable<Unordered>),
    UnreliableSequenced(Unreliable<Sequenced>),
    ReliableUnordered(Reliable<Unordered>),
    ReliableSequenced(Reliable<Sequenced>),
    ReliableOrdered(Reliable<Ordered>),
}
