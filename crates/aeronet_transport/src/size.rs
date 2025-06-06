//! See [`MinSize`].

use {
    bevy_platform::{
        collections::{HashMap, HashSet},
        time::Instant,
    },
    bevy_reflect::Reflect,
    bit_vec::BitVec,
    core::num::TryFromIntError,
    octs::{BufTooShortOr, Decode, Encode, EncodeLen, FixedEncodeLenHint, Read, VarInt, Write},
    typesize::{TypeSize, derive::TypeSize},
};

#[cfg(target_pointer_width = "16")]
compile_error!("`aeronet_transport` cannot safely compile for a 16-bit platform");

/// Smallest pointer-sized integer supported by this crate.
///
/// This crate does not support any platforms where [`usize`] is smaller than
/// [`u32`]. We do some casts between [`usize`] and [`u32`] when encoding and
/// decoding packets, which would be incorrect (but not technically unsafe) if
/// [`usize`] was smaller.
///
/// However, this restriction is fine, since `bevy_ecs` also won't compile for
/// any platforms where `usize` is smaller than [`u32`].
///
/// This value is always encoded as a [`VarInt`] on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, TypeSize, Reflect)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MinSize(pub u32);

impl MinSize {
    /// Largest value that can be represented by this type.
    pub const MAX: Self = Self(u32::MAX);
}

impl From<u8> for MinSize {
    fn from(value: u8) -> Self {
        Self(u32::from(value))
    }
}

impl From<u16> for MinSize {
    fn from(value: u16) -> Self {
        Self(u32::from(value))
    }
}

impl From<u32> for MinSize {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl TryFrom<usize> for MinSize {
    type Error = TryFromIntError;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        u32::try_from(value).map(Self)
    }
}

impl From<MinSize> for usize {
    fn from(value: MinSize) -> Self {
        value.0 as Self
    }
}

impl FixedEncodeLenHint for MinSize {
    const MIN_ENCODE_LEN: usize = <VarInt<u32> as FixedEncodeLenHint>::MIN_ENCODE_LEN;

    const MAX_ENCODE_LEN: usize = <VarInt<u32> as FixedEncodeLenHint>::MAX_ENCODE_LEN;
}

impl EncodeLen for MinSize {
    fn encode_len(&self) -> usize {
        VarInt(self.0).encode_len()
    }
}

impl Encode for MinSize {
    type Error = <VarInt<u32> as Encode>::Error;

    fn encode(&self, mut dst: impl Write) -> Result<(), BufTooShortOr<Self::Error>> {
        dst.write(VarInt(self.0))
    }
}

impl Decode for MinSize {
    type Error = <VarInt<u32> as Decode>::Error;

    fn decode(mut src: impl Read) -> Result<Self, BufTooShortOr<Self::Error>> {
        Ok(Self(src.read::<VarInt<u32>>()?.0))
    }
}

pub(crate) const fn of_instant(_v: &Instant) -> usize {
    0
}

pub(crate) fn of_bitvec(v: &BitVec) -> usize {
    v.capacity() / 8
}

pub(crate) fn of_set<T: TypeSize>(v: &HashSet<T>) -> usize {
    // copied from
    // <https://github.com/GnomedDev/typesize/blob/2be34b451154dbcd257e36fe661e8a93e73b3fa6/src/vec.rs#L5>
    v.iter().map(TypeSize::get_size).sum::<usize>() + (v.capacity() - v.len()) * size_of::<T>()
}

pub(crate) fn of_map<K: TypeSize, V: TypeSize>(v: &HashMap<K, V>) -> usize {
    // copied from
    // <https://github.com/GnomedDev/typesize/blob/2be34b451154dbcd257e36fe661e8a93e73b3fa6/src/map.rs#L15>
    let element_size = v
        .iter()
        .map(|(k, v)| k.get_size() + v.get_size())
        .sum::<usize>();
    let free_size = (v.capacity() - v.len()) * (size_of::<K>() + size_of::<V>());
    element_size + free_size
}
