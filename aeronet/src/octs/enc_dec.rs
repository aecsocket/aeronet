use bytes::{Buf, Bytes};
use integer_encoding::VarInt;

use super::{ConstEncodeLen, Decode, Encode, EncodeLen, ReadBytes, Result, WriteBytes};

macro_rules! impl_uint {
    ($ty:ty, $read:ident, $write:ident, $width:literal) => {
        impl Encode for $ty {
            fn encode(&self, buf: &mut impl WriteBytes) -> Result<()> {
                buf.$write(*self)
            }
        }

        impl ConstEncodeLen for $ty {
            const ENCODE_LEN: usize = $width;
        }

        impl Decode for $ty {
            fn decode(buf: &mut impl ReadBytes) -> Result<Self> {
                buf.$read()
            }
        }
    };
}

impl_uint!(u8, read_u8, write_u8, 1);
impl_uint!(u16, read_u16, write_u16, 2);
impl_uint!(u32, read_u32, write_u32, 4);
impl_uint!(u64, read_u64, write_u64, 8);

impl EncodeLen for Bytes {
    fn encode_len(&self) -> usize {
        VarInt::required_space(self.len()) + self.len()
    }
}

impl Encode for Bytes {
    fn encode(&self, buf: &mut impl WriteBytes) -> Result<()> {
        buf.write_varint_usize(self.len())?;
        buf.write_slice(self.chunk())?;
        Ok(())
    }
}

impl Decode for Bytes {
    fn decode(buf: &mut impl ReadBytes) -> Result<Self> {
        let len = buf.read_varint_usize()?;
        buf.read_slice(len)
    }
}
