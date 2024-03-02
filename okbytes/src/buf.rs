use crate::Result;

pub trait ReadBuf: Sized {
    type Slice;

    fn read_u8(&mut self) -> Result<u8>;

    fn read_u16(&mut self) -> Result<u16>;

    fn read_u32(&mut self) -> Result<u32>;

    fn read_u64(&mut self) -> Result<u64>;

    fn read_varint(&mut self) -> Result<u64>;

    fn read_slice(&mut self, len: usize) -> Result<Self::Slice>;

    fn read<T: Decode>(&mut self) -> Result<T> {
        T::decode(self)
    }
}

pub trait WriteBuf: ReadBuf {
    fn write_u8(&mut self, value: u8) -> Result<usize>;

    fn write_u16(&mut self, value: u16) -> Result<usize>;

    fn write_u32(&mut self, value: u32) -> Result<usize>;

    fn write_u64(&mut self, value: u64) -> Result<usize>;

    fn write_varint(&mut self, value: u64) -> Result<usize>;

    fn write_slice(&mut self, src: &[u8]) -> Result<usize>;

    fn write<T: Encode>(&mut self, value: &T) -> Result<usize> {
        value.encode(self)
    }
}

pub trait Decode: Sized {
    fn decode(buf: &mut impl ReadBuf) -> Result<Self>;
}

pub trait Encode {
    fn encode(&self, buf: &mut impl WriteBuf) -> Result<usize>;
}

macro_rules! impl_encode_decode {
    ($ty:ty, $read:ident, $write:ident) => {
        impl Decode for $ty {
            fn decode(buf: &mut impl ReadBuf) -> Result<Self> {
                buf.$read()
            }
        }

        impl Encode for $ty {
            fn encode(&self, buf: &mut impl WriteBuf) -> Result<usize> {
                buf.$write(*self)
            }
        }
    };
}

impl_encode_decode!(u8, read_u8, write_u8);
impl_encode_decode!(u16, read_u16, write_u16);
impl_encode_decode!(u32, read_u32, write_u32);
impl_encode_decode!(u64, read_u64, write_u64);
