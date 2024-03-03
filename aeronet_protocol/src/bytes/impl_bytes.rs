use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::bytes::varint;

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
            let mut result: u64 = 0;
            for times_shift in 0..8 {
                let byte = self.read_u8()?;
                let msb_dropped = byte & 0b0111_1111;
                result |= (msb_dropped as u64) << times_shift * 7;

                if byte & 0b1000_0000 == 0 {
                    return Ok(result);
                }
            }
            Err(BytesError::BufferTooShort)
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

impl WriteBytes for bytes::BytesMut {
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
        let len = varint::size_of(v);
        if self.remaining_mut() < len {
            return Err(BytesError::BufferTooShort);
        }

        let buf = match len {
            1 => self.write_u8(v as u8)?,
            2 => {
                let buf = self.write_u16(v as u16)?;
                buf[0] |= 0x40;
                buf
            }
            4 => {
                let buf = self.write_u32(v as u32)?;
                buf[0] |= 0x80;
                buf
            }
            8 => {
                let buf = self.write_u64(v)?;
                buf[0] |= 0xc0;
                buf
            }
            _ => return Err(BytesError::VarInt),
        };

        Ok(buf)
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
