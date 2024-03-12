use bytes::{Buf, BufMut, Bytes, BytesMut};
use integer_encoding::VarInt;

use super::{BytesError, ReadBytes, Result, WriteBytes};

macro_rules! read_u {
    ($self:expr, $ty:ty, $get:ident, $size:literal) => {
        if $self.remaining() >= $size {
            Ok($self.$get())
        } else {
            Err(BytesError::BufferTooShort)
        }
    };
}

macro_rules! write_u {
    ($self:expr, $value:expr, $ty:ty, $put:ident, $size:literal) => {
        if $self.remaining_mut() >= $size {
            $self.$put($value);
            Ok(())
        } else {
            Err(BytesError::BufferTooShort)
        }
    };
}

macro_rules! impl_read {
    () => {
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
            let (value, bytes_read) =
                <u64 as VarInt>::decode_var(self.chunk()).ok_or(BytesError::BufferTooShort)?;
            self.advance(bytes_read);
            Ok(value)
        }
    };
}

impl ReadBytes for Bytes {
    impl_read!();

    #[inline]
    fn read_slice(&mut self, len: usize) -> Result<Bytes> {
        if self.remaining() > len {
            Ok(self.slice(..len))
        } else {
            Err(BytesError::BufferTooShort)
        }
    }
}

impl ReadBytes for BytesMut {
    impl_read!();

    #[inline]
    fn read_slice(&mut self, len: usize) -> Result<Bytes> {
        if self.remaining() > len {
            Ok(self.clone().freeze().slice(..len))
        } else {
            Err(BytesError::BufferTooShort)
        }
    }
}

impl WriteBytes for BytesMut {
    #[inline]
    fn write_u8(&mut self, value: u8) -> Result<()> {
        write_u!(self, value, u8, put_u8, 1)
    }

    #[inline]
    fn write_u16(&mut self, value: u16) -> Result<()> {
        write_u!(self, value, u16, put_u16, 2)
    }

    #[inline]
    fn write_u32(&mut self, value: u32) -> Result<()> {
        write_u!(self, value, u32, put_u32, 4)
    }

    #[inline]
    fn write_u64(&mut self, value: u64) -> Result<()> {
        write_u!(self, value, u64, put_u64, 8)
    }

    #[inline]
    fn write_varint(&mut self, v: u64) -> Result<()> {
        let len = VarInt::required_space(v);
        if self.remaining_mut() < len {
            return Err(BytesError::BufferTooShort);
        }
        VarInt::encode_var(v, &mut self[..len]);
        Ok(())
    }

    #[inline]
    fn write_slice(&mut self, src: &[u8]) -> Result<()> {
        if self.remaining_mut() >= src.len() {
            self.put_slice(src);
            Ok(())
        } else {
            Err(BytesError::BufferTooShort)
        }
    }
}
