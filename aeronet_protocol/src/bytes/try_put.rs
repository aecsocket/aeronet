use bytes::BufMut;

#[derive(Debug, Clone, thiserror::Error)]
#[error("buffer too short")]
pub struct BytesWriteError;

pub trait TryPutExt {
    fn try_put_slice(&mut self, src: &[u8]) -> Result<(), BytesWriteError>;

    fn try_put_u8(&mut self, n: u8) -> Result<(), BytesWriteError>;

    fn try_put_u16(&mut self, n: u16) -> Result<(), BytesWriteError>;

    fn try_put_u32(&mut self, n: u32) -> Result<(), BytesWriteError>;

    fn try_put_u64(&mut self, n: u64) -> Result<(), BytesWriteError>;

    fn try_put_varint(&mut self, n: u64) -> Result<(), BytesWriteError>;
}

impl<T: BufMut> TryPutExt for T {
    #[inline]
    fn try_put_slice(&mut self, src: &[u8]) -> Result<(), BytesWriteError> {
        if self.remaining_mut() < src.len() {
            Err(BytesWriteError)
        } else {
            self.put_slice(src);
            Ok(())
        }
    }

    #[inline]
    fn try_put_u8(&mut self, n: u8) -> Result<(), BytesWriteError> {
        self.try_put_slice(&[n])
    }

    #[inline]
    fn try_put_u16(&mut self, n: u16) -> Result<(), BytesWriteError> {
        self.try_put_slice(&n.to_be_bytes())
    }

    #[inline]
    fn try_put_u32(&mut self, n: u32) -> Result<(), BytesWriteError> {
        self.try_put_slice(&n.to_be_bytes())
    }

    #[inline]
    fn try_put_u64(&mut self, n: u64) -> Result<(), BytesWriteError> {
        self.try_put_slice(&n.to_be_bytes())
    }

    #[inline]
    fn try_put_varint(&mut self, mut n: u64) -> Result<(), BytesWriteError> {
        while n >= 0x80 {
            self.try_put_u8(((n & 0x7F) | 0x80) as u8)?;
            n >>= 7;
        }
        self.try_put_u8(n as u8)?;
        Ok(())
    }
}
