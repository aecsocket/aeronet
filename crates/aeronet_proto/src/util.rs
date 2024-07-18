use std::{
    cmp::Ordering,
    ops::{Add, AddAssign, Deref, DerefMut, Sub, SubAssign},
};

use octs::{BufTooShortOr, Bytes, Decode, Encode, FixedEncodeLen, Read, Write};

use crate::{
    frag::FragmentMarker,
    packet::{MessageSeq, PacketSeq},
    seq::Seq,
};

macro_rules! forward_deref {
    ($ty:ty, $base:ty) => {
        impl Deref for $ty {
            type Target = $base;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl DerefMut for $ty {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.0
            }
        }
    };
}

macro_rules! forward_arithmetic {
    ($ty:ty) => {
        impl Ord for $ty {
            fn cmp(&self, other: &Self) -> Ordering {
                self.0.cmp(&other.0)
            }
        }

        impl PartialOrd for $ty {
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                Some(self.cmp(other))
            }
        }

        impl Add for $ty {
            type Output = Self;

            fn add(self, rhs: Self) -> Self::Output {
                Self(self.0 + rhs.0)
            }
        }

        impl AddAssign for $ty {
            fn add_assign(&mut self, rhs: Self) {
                *self = *self + rhs;
            }
        }

        impl Sub for $ty {
            type Output = Self;

            fn sub(self, rhs: Self) -> Self::Output {
                Self(self.0 - rhs.0)
            }
        }

        impl SubAssign for $ty {
            fn sub_assign(&mut self, rhs: Self) {
                *self = *self - rhs;
            }
        }
    };
}

macro_rules! forward_encode_decode {
    ($ty:ty, $base:ty) => {
        impl Encode for $ty {
            type Error = <$base as Encode>::Error;

            fn encode(&self, dst: impl Write) -> Result<(), BufTooShortOr<Self::Error>> {
                self.0.encode(dst)
            }
        }

        impl Decode for $ty {
            type Error = <$base as Decode>::Error;

            fn decode(src: impl Read) -> Result<Self, BufTooShortOr<Self::Error>> {
                <$base>::decode(src).map(Self)
            }
        }
    };
}

macro_rules! forward_fixed_encode_len {
    ($ty:ty, $base:ty) => {
        impl FixedEncodeLen for $ty {
            const ENCODE_LEN: usize = <$base as FixedEncodeLen>::ENCODE_LEN;
        }
    };
}

forward_deref!(MessageSeq, Seq);
forward_arithmetic!(MessageSeq);
forward_encode_decode!(MessageSeq, Seq);
forward_fixed_encode_len!(MessageSeq, Seq);

forward_deref!(PacketSeq, Seq);
forward_arithmetic!(PacketSeq);
forward_encode_decode!(PacketSeq, Seq);
forward_fixed_encode_len!(PacketSeq, Seq);

forward_encode_decode!(FragmentMarker, u8);
forward_fixed_encode_len!(FragmentMarker, u8);

pub(crate) fn bytes_data_size(value: &Bytes) -> usize {
    value.len()
}
