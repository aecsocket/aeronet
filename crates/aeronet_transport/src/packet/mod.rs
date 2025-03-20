//! Defines the structure of packets on the wire.
//!
//! This module only contains the type definitions themselves, to make it easy
//! to understand the whole protocol at a glance, and to have centralized
//! documentation on how the protocol works. The actual logic is implemented in
//! different modules.
//!
//! This file should be ordered in a way that makes it easy to understand the
//! protocol when reading top-to-bottom.
//!
//! The layout of a single packet is:
//!
//! ```rust,ignore
//! struct Packet {
//!     header: PacketHeader,
//!     fragments: [MessageFragment],
//! }
//! ```
//!
//! This is not defined as a struct since we don't read all fragments in advance
//! and then process them; that would require pointlessly allocating a [`Vec`]
//! to store the fragments. Instead, the logic looks like:
//!
//! ```rust,ignore
//! fn process_packet(packet: &[u8]) {
//!     process_header(&mut packet);
//!     while !packet.is_empty() {
//!         process_fragment(&mut packet);
//!     }
//! }
//! ```

mod ack;
mod frag;
mod header;
mod payload;
mod seq;

pub use payload::*;
use {
    crate::{lane::LaneIndex, min_size::MinSize},
    bevy_reflect::Reflect,
    derive_more::{Add, AddAssign, Deref, DerefMut, Sub, SubAssign},
    octs::Bytes,
    typesize::derive::TypeSize,
};

/// Sequence number uniquely identifying an item sent across a network.
///
/// Note that the sequence number may identify either a message or a packet
/// sequence number - see [`MessageSeq`] and [`PacketSeq`].
///
/// The number is stored internally as a [`u16`], which means it will wrap
/// around fairly quickly as many messages and packets can be sent per second.
/// Users of a sequence number should take this into account, and use the custom
/// [`Seq::cmp`] implementation which takes wraparound into consideration.
///
/// # Wraparound
///
/// Operations on [`Seq`] must take into account wraparound, as it is inevitable
/// that it will eventually occur in the program - a [`u16`] is relatively very
/// small.
///
/// The sequence number can be visualized as an infinite number line, where
/// [`u16::MAX`] is right before `0`, `0` is before `1`, etc.:
///
/// ```text
///     65534  65535    0      1      2
/// ... --|------|------|------|------|-- ...
/// ```
///
/// [Addition](std::ops::Add) and [subtraction](std::ops::Sub) will always wrap.
///
/// See <https://gafferongames.com/post/packet_fragmentation_and_reassembly/>, *Fragment Packet Structure*.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, TypeSize, Reflect)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Seq(pub u16);

/// Sequence number of a packet in transit.
///
/// This is used in [`PacketHeader`] for tracking packet-level acknowledgements
/// (see [`Acknowledge`]).
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, TypeSize)] // force `#[derive]` on multiple lines
#[derive(Deref, DerefMut, Add, AddAssign, Sub, SubAssign, Reflect)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PacketSeq(pub Seq);

/// Sequence number of a message in transit.
///
/// This is used for fragmentation, reassembly, reliability, and ordering.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, TypeSize)] // force `#[derive]` on multiple lines
#[derive(Deref, DerefMut, Add, AddAssign, Sub, SubAssign, Reflect)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MessageSeq(pub Seq);

/// Receiver-side data structure for tracking which packets, that a sender has
/// sent, have been successfully received by the receiver (that the receiver has
/// *acknowledged* that they've received).
///
/// This uses a modification of the strategy described in [*Gaffer On Games*],
/// where we store two pieces of info:
/// * the last received packet sequence number (`last_recv`)
/// * a bitfield of which packets before `last_recv` have been acked (`bits`)
///
/// If a bit at index `N` is set in `bits`, then the packet with sequence
/// `last_recv - N` has been acked. For example,
///
/// ```text
/// last_recv: 40
///      bits: 0b0000..00001001
///                    ^   ^  ^
///                    |   |  +- seq 40 (40 - 0) has been acked
///                    |   +---- seq 37 (40 - 3) has been acked
///                    +-------- seq 33 (40 - 7) has NOT been acked
/// ```
///
/// This info is sent with every packet, and the last 32 packet acknowledgements
/// are sent, giving a lot of reliability and redundancy for acks.
///
/// [*Gaffer On Games*]: https://gafferongames.com/post/reliable_ordered_messages/#packet-levelacks
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, TypeSize, Reflect)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Acknowledge {
    /// Last packet sequence number that the receiver received.
    pub last_recv: PacketSeq,
    /// Bitfield of which packets before and including `last_recv` have been
    /// acknowledged.
    pub bits: u32,
}

/// Metadata for a single packet.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, TypeSize, Reflect)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PacketHeader {
    /// Monotonically increasing sequence number of this packet.
    pub seq: PacketSeq,
    /// Informs the receiver which packets the sender has already received.
    pub acks: Acknowledge,
}

/// Marks the index and last state of a single fragment.
///
/// This type serves two purposes - it defines the index of the fragment that we
/// are sending over (since fragments may be received out of order), and defines
/// whether this fragment is the last one in this message.
///
/// To achieve this while still being efficient:
/// - the position is encoded as a varint
///   - most messages should be smaller than 128 fragments, so there should be
///     minimal overhead in most cases
/// - even values are considered *non-last*
/// - odd values are considered *last*
///
/// - `0`: non-last
/// - `1`: last
/// - `2`: non-last
/// - ...
#[derive(Clone, Copy, PartialEq, Eq, Hash, TypeSize, Reflect)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FragmentPosition(MinSize);

/// Wrapper for the actual contents of a [`Fragment`].
///
/// On the wire, this is encoded as a varint of how long the payload is, plus
/// the actual payload.
///
/// The length of the underlying byte buffer must not exceed
/// [`MinSize::MAX`], or it cannot be encoded.
#[derive(Debug, Clone, PartialEq, Eq, Deref, DerefMut)]
pub struct FragmentPayload(pub Bytes);

/// Front-loaded [`Fragment`] metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TypeSize, Reflect)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FragmentHeader {
    /// Lane index of this fragment's message, relative to the receiver's
    /// receive lanes.
    pub lane: LaneIndex,
    /// Monotonically increasing sequence number of this message.
    ///
    /// The message sequence number is only guaranteed to be monotonically
    /// increasing *on this specific lane*.
    pub seq: MessageSeq,
    /// Position of the fragment that we are about to deliver.
    pub position: FragmentPosition,
}

/// Single fragment of a message.
#[derive(Debug, Clone)]
pub struct Fragment {
    /// Fragment metadata.
    pub header: FragmentHeader,
    /// User-defined data to be delivered.
    pub payload: FragmentPayload,
}
