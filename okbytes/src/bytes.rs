use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::{BufferTooShort, ReadBuf, Result, WriteBuf};

macro_rules! read_u {
    ($self:expr, $ty:ty, $get:ident, $size:literal) => {
        if $self.remaining() >= $size {
            Ok($self.$get())
        } else {
            Err(BufferTooShort)
        }
    };
}

macro_rules! write_u {
    ($self:expr, $value:expr, $ty:ty, $put:ident, $size:literal) => {
        if $self.remaining_mut() >= $size {
            $self.$put($value);
            Ok($size)
        } else {
            Err(BufferTooShort)
        }
    };
}

macro_rules! impl_read {
    () => {
        type Slice = Bytes;

        #[inline]
        fn read_u8(&mut self) -> Result<u8> {
            read_u!(self, u8, get_u8, 1)
        }

        #[inline]
        fn read_u16(&mut self) -> Result<u16> {
            read_u!(self, u16, get_u16, 2)
        }

        #[inline]
        fn read_u32(&mut self) -> Result<u32> {
            read_u!(self, u32, get_u32, 4)
        }

        #[inline]
        fn read_u64(&mut self) -> Result<u64> {
            read_u!(self, u64, get_u64, 8)
        }

        #[inline]
        fn read_varint(&mut self) -> Result<u64> {
            todo!()
        }
    };
}

impl ReadBuf for Bytes {
    impl_read!();

    #[inline]
    fn read_slice(&mut self, len: usize) -> Result<Self::Slice> {
        if self.remaining() > len {
            Ok(self.slice(..len))
        } else {
            Err(BufferTooShort)
        }
    }
}

impl ReadBuf for BytesMut {
    impl_read!();

    #[inline]
    fn read_slice(&mut self, len: usize) -> Result<Self::Slice> {
        if self.remaining() > len {
            Ok(self.clone().freeze().slice(..len))
        } else {
            Err(BufferTooShort)
        }
    }
}

#[cfg(feature = "bytes")]
impl WriteBuf for bytes::BytesMut {
    #[inline]
    fn write_u8(&mut self, value: u8) -> Result<usize> {
        write_u!(self, value, u8, put_u8, 1)
    }

    #[inline]
    fn write_u16(&mut self, value: u16) -> Result<usize> {
        write_u!(self, value, u16, put_u16, 2)
    }

    #[inline]
    fn write_u32(&mut self, value: u32) -> Result<usize> {
        write_u!(self, value, u32, put_u32, 4)
    }

    #[inline]
    fn write_u64(&mut self, value: u64) -> Result<usize> {
        write_u!(self, value, u64, put_u64, 8)
    }

    #[inline]
    fn write_varint(&mut self, value: u64) -> Result<usize> {
        todo!()
    }

    #[inline]
    fn write_slice(&mut self, src: &[u8]) -> Result<usize> {
        if self.remaining_mut() >= src.len() {
            self.put_slice(src);
            Ok(src.len())
        } else {
            Err(BufferTooShort)
        }
    }
}
