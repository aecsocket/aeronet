mod ack;
mod frag;
mod header;
mod payload;
mod seq;

pub use payload::*;
use static_assertions::const_assert;
use typesize::derive::TypeSize;
use {
    crate::lane::LaneIndex,
    arbitrary::Arbitrary,
    bevy_derive::{Deref, DerefMut},
    bevy_reflect::Reflect,
    derive_more::{Add, AddAssign, Sub, SubAssign},
    octs::Bytes,
    std::mem::size_of,
};

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Arbitrary, TypeSize, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Seq(pub u16);

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Arbitrary, TypeSize, Reflect)] // force `#[derive]` on multiple lines
#[derive(Deref, DerefMut, Add, AddAssign, Sub, SubAssign)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PacketSeq(pub Seq);

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Arbitrary, TypeSize, Reflect)] // force `#[derive]` on multiple lines
#[derive(Deref, DerefMut, Add, AddAssign, Sub, SubAssign)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MessageSeq(pub Seq);

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Arbitrary, TypeSize, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Acknowledge {
    pub last_recv: PacketSeq,
    pub bits: u32,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Arbitrary, TypeSize, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PacketHeader {
    pub seq: PacketSeq,
    pub acks: Acknowledge,
}

#[derive(Debug, Clone, PartialEq, Eq, Deref, DerefMut)]
pub struct MessagePayload(pub Bytes);

pub struct MessageFragment {
    pub seq: MessageSeq,
    pub lane: LaneIndex,
    pub pos: FragmentPosition,
    pub payload: MessagePayload,
}

pub type FragmentIndex = u64;

const_assert!(size_of::<usize>() >= size_of::<FragmentIndex>());

#[derive(Clone, Copy, PartialEq, Eq, Hash, Arbitrary, TypeSize, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FragmentPosition(FragmentIndex);

/*
packet examples:

```ignore
PacketHeader {
    seq: 1234,
    acks: Acknowledge {
        last_recv: 1200,
        bits: 0b1111..1011,
    },
}

MessageFragment {
    seq: 50,
    lane: 0,
    pos: FragmentPosition {
        index: 0,
        is_last: true,
    },
    payload: b"hello world",
}

MessageFragment {
    seq: 51,
    lane: 0,
    pos: FragmentPosition {
        index: 0,
        is_last: false,
    },
    payload: b"goodbye ",
}

MessageFragment {
    seq: 51,
    lane: 0,
    pos: FragmentPosition {
        index: 1,
        is_last: true,
    },
    payload: b"world",
}
```
*/
