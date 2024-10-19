use bevy_reflect::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LaneReliability {
    Unreliable,
    Reliable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
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
