use {
    arbitrary::Arbitrary,
    bevy_reflect::prelude::*,
    octs::{BufTooShortOr, Decode, Encode, EncodeLen, Read, VarInt, Write},
    static_assertions::const_assert,
    typesize::derive::TypeSize,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TypeSize, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum LaneKind {
    UnreliableUnordered,
    UnreliableSequenced,
    ReliableUnordered,
    ReliableOrdered,
}

impl LaneKind {
    #[must_use]
    pub const fn reliability(&self) -> LaneReliability {
        match self {
            Self::UnreliableUnordered | Self::UnreliableSequenced => LaneReliability::Unreliable,
            Self::ReliableUnordered | Self::ReliableOrdered => LaneReliability::Reliable,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TypeSize, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum LaneReliability {
    Unreliable,
    Reliable,
}

type RawLaneIndex = u32;

const_assert!(size_of::<usize>() >= size_of::<RawLaneIndex>());

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Arbitrary, TypeSize, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LaneIndex(RawLaneIndex);

impl LaneIndex {
    #[must_use]
    pub const fn from_raw(index: RawLaneIndex) -> Self {
        Self(index)
    }

    #[must_use]
    pub const fn into_raw(self) -> RawLaneIndex {
        self.0
    }

    #[must_use]
    pub const fn into_usize(self) -> usize {
        // `RawLaneIndex` is checked to be at least `usize` bits at compile time
        self.into_raw() as usize
    }
}

impl EncodeLen for LaneIndex {
    fn encode_len(&self) -> usize {
        VarInt(self.0).encode_len()
    }
}

impl Encode for LaneIndex {
    type Error = <VarInt<u32> as Encode>::Error;

    fn encode(&self, mut dst: impl Write) -> Result<(), BufTooShortOr<Self::Error>> {
        dst.write(VarInt(self.0))
    }
}

impl Decode for LaneIndex {
    type Error = <VarInt<u32> as Decode>::Error;

    fn decode(mut src: impl Read) -> Result<Self, BufTooShortOr<Self::Error>> {
        Ok(Self(src.read::<VarInt<u32>>()?.0))
    }
}
