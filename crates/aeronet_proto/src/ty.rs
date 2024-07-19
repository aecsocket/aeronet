//! Definitions for types sent by the protocol level.
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
//!     fragments: [Fragment],
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

use aeronet::lane::LaneIndex;
use arbitrary::Arbitrary;
use datasize::DataSize;
use derivative::Derivative;
use derive_more::{Add, AddAssign, Deref, DerefMut, From, Sub, SubAssign};
use octs::Bytes;

/// Sequence number uniquely identifying an item sent across a network.
///
/// Note that the sequence number may identify either a message or a packet
/// sequence number - see [`MessageSeq`] and [`PacketSeq`].
///
/// The number is stored internally as a [`u16`], which means it will wrap
/// around fairly quickly as many messages and packetscan be sent per second.
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
#[derive(Derivative, Clone, Copy, Default, PartialEq, Eq, Hash, Arbitrary, DataSize)]
#[derivative(Debug = "transparent")]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Seq(pub u16);

/// Metadata for a packet sent and received by the protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Arbitrary, DataSize)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PacketHeader {
    /// Monotonically increasing sequence number of this packet.
    pub seq: PacketSeq,
    /// Informs the receiver which packets the sender has already received.
    pub acks: Acknowledge,
}

/// Sequence number of a packet in transit.
///
/// This is used in [`PacketHeader`] for tracking packet-level acknowledgements
/// (see [`Acknowledge`]).
#[derive(
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    From,
    Deref,
    DerefMut,
    Add,
    AddAssign,
    Sub,
    SubAssign,
    Arbitrary,
    DataSize,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PacketSeq(pub Seq);

/// Sequence number of a message in transit.
///
/// This is used in [`FragmentHeader`] for fragmentation and reassembly (see
/// [`frag`]), and reliability and ordering (see [`session`]).
///
/// [`frag`]: crate::frag
/// [`session`]: crate::session
#[derive(
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    From,
    Deref,
    DerefMut,
    Add,
    AddAssign,
    Sub,
    SubAssign,
    Arbitrary,
    DataSize,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MessageSeq(pub Seq);

/// Receiver-side data structure for tracking which packets, that a sender has
/// sent, have been successfully received by the receiver (that the receiver has
/// *acknowledged* that they've received).
///
/// This uses a modification of the strategy described in [*Gaffer On Games*],
/// where we store two pieces of info:
/// * the last received packet sequence number (`last_recv`)
/// * a bitfield of which packets before `last_recv` have been acked
///   (`ack_bits`)
///
/// If a bit at index `N` is set in `ack_bits`, then the packet with sequence
/// `last_recv - N` has been acked. For example,
///
/// ```text
/// last_recv: 40
///  ack_bits: 0b0000..00001001
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
#[derive(Derivative, Clone, Copy, Default, PartialEq, Eq, Hash, Arbitrary, DataSize)]
#[derivative(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Acknowledge {
    /// Last received packet sequence number.
    pub last_recv: PacketSeq,
    /// Bitfield of which packets before `last_recv` have been acknowledged.
    #[derivative(Debug(format_with = "crate::ack::fmt"))]
    pub ack_bits: u32,
}

/// Part of, or potentially the entirety of, a user-sent message, along with
/// metadata.
#[derive(Debug, Clone, PartialEq, Eq, DataSize)]
pub struct Fragment {
    /// Front-loadead metadata.
    pub header: FragmentHeader,
    /// Buffer storing the user-defined message payload of this fragment.
    ///
    /// On the wire, this is stored as a [`VarInt`] defining the payload length,
    /// followed by that many bytes of payload.
    ///
    /// [`VarInt`]: octs::VarInt
    #[data_size(with = Bytes::len)]
    pub payload: Bytes,
}

/// Metadata for a [`Fragment`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Arbitrary, DataSize)]
pub struct FragmentHeader {
    /// Index of the lane on which this fragment must be received.
    ///
    /// This is the *receiver-side* lane index. If we have the following lanes:
    /// - client to server: [A, B]
    /// - server to client: [C]
    ///
    /// If the server sends a message and wants it to end up in lane B, it must
    /// specify lane index 1.
    ///
    /// On the wire, this is encoded as a [`VarInt`].
    ///
    /// [`VarInt`]: octs::VarInt
    #[data_size(skip)]
    pub lane_index: LaneIndex,
    /// Monotonically increasing sequence number of the message that this
    /// fragment is a part of.
    ///
    /// Message sequence numbers are only monotonically increasing relative to
    /// a specific lane. For example, you may be sending messages 10, 11, 12 on
    /// lane 0, while also sending messages 3, 4, 5 on lane 1.
    pub msg_seq: MessageSeq,
    /// Marker of this fragment, indicating the fragment's index, and whether it
    /// is the last fragment of this message or not.
    pub marker: FragmentMarker,
}

/// Indicates what index a [`Fragment`] represents, and whether this fragment
/// is the last fragment in a message.
///
/// When transmitting fragments to a peer, we need some way to tell if we have
/// received all of the fragments for a specific message. [*Gaffer On Games*]
/// uses two [`u8`]s, a `fragment id` and `num fragments`, to represent this
/// data. However, we do something smarter and use the MSB to indicate if this
/// fragment is the last one in the message. This leaves us with 128 possible
/// fragments per message, which should still be enough for most reasonable
/// use cases, but saves 1 byte of overhead per fragment per packet.
///
/// If the MSB is set, this fragment is the last one in this message. The other
/// 7 bits encode the index of this fragment in the message.
///
/// [*Gaffer On Games*]: https://gafferongames.com/post/packet_fragmentation_and_reassembly/#fragment-packet-structure
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Arbitrary, DataSize)]
pub struct FragmentMarker(pub(crate) u8);
