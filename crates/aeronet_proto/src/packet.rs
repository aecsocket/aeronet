//! Type definitions for packet-level types.

use std::{convert::Infallible, fmt::Debug};

use octs::{BufTooShortOr, Decode, Encode, FixedEncodeLen, Read, Write};

use crate::{ack::Acknowledge, seq::Seq};

/// Sequence number of a packet in transit.
///
/// This is used in packet acknowledgements (see [`Acknowledge`]).
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, arbitrary::Arbitrary, datasize::DataSize)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PacketSeq(pub Seq);

impl Debug for PacketSeq {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0 .0)
    }
}

impl PacketSeq {
    /// Creates a new sequence number from a raw number.
    ///
    /// If you already have a [`Seq`], just wrap it in a [`PacketSeq`].
    #[must_use]
    pub const fn new(n: u16) -> Self {
        Self(Seq(n))
    }
}

/// Sequence number of a message in transit.
///
/// This is used in packet fragmentation and reassembly (see [`frag`]).
///
/// [`frag`]: crate::frag
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, arbitrary::Arbitrary, datasize::DataSize)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MessageSeq(pub Seq);

impl Debug for MessageSeq {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0 .0)
    }
}

impl MessageSeq {
    /// Creates a new sequence number from a raw number.
    ///
    /// If you already have a [`Seq`], just wrap it in a [`MessageSeq`].
    #[must_use]
    pub const fn new(n: u16) -> Self {
        Self(Seq(n))
    }
}

/// Header data for a single packet emitted by [`Session`].
///
/// [`Session`]: crate::session::Session
#[derive(Debug, Clone, Copy, PartialEq, Eq, arbitrary::Arbitrary)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PacketHeader {
    /// Sequence number of this packet.
    pub packet_seq: PacketSeq,
    /// Informs the peer which packets this side has already received.
    pub acks: Acknowledge,
}

impl FixedEncodeLen for PacketHeader {
    const ENCODE_LEN: usize = PacketSeq::ENCODE_LEN + Acknowledge::ENCODE_LEN;
}

impl Encode for PacketHeader {
    type Error = Infallible;

    fn encode(&self, mut dst: impl Write) -> Result<(), BufTooShortOr<Self::Error>> {
        dst.write(self.packet_seq)?;
        dst.write(self.acks)?;
        Ok(())
    }
}

impl Decode for PacketHeader {
    type Error = Infallible;

    fn decode(mut src: impl Read) -> Result<Self, BufTooShortOr<Self::Error>> {
        Ok(Self {
            packet_seq: src.read()?,
            acks: src.read()?,
        })
    }
}
