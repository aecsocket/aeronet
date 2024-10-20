use {
    bevy_reflect::prelude::*,
    datasize::DataSize,
    octs::{BufTooShortOr, Decode, Encode, EncodeLen, Read, VarInt, Write},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, DataSize, Reflect)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, DataSize, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum LaneReliability {
    Unreliable,
    Reliable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, DataSize, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LaneIndex(u32);

impl LaneIndex {
    #[must_use]
    pub const fn from_raw(index: u32) -> Self {
        Self(index)
    }

    #[must_use]
    pub const fn into_raw(self) -> u32 {
        self.0
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
