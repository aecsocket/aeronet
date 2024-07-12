use std::{
    convert::Infallible,
    ops::{Deref, DerefMut},
};

use aeronet::lane::LaneIndex;
use octs::{
    BufTooShortOr, Decode, Encode, EncodeLen, FixedEncodeLen, FixedEncodeLenHint, Read, VarInt,
    VarIntTooLarge, Write,
};

use crate::{ack::Acknowledge, seq::Seq};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, arbitrary::Arbitrary)]
pub struct PacketSeq(pub Seq);

impl PacketSeq {
    pub const fn new(n: u16) -> Self {
        Self(Seq(n))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, arbitrary::Arbitrary)]
pub struct MessageSeq(pub Seq);

impl MessageSeq {
    pub const fn new(n: u16) -> Self {
        Self(Seq(n))
    }
}
