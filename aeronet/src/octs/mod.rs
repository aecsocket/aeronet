//! Utilities for working with [`Bytes`].
//!
//! Who decided to make the [`Bytes`] API panic on failure?!?!?!?!

use bytes::Bytes;

mod chunks;
mod enc_dec;
mod impl_bytes;

pub use chunks::*;

/// Error when reading or writing to/from a byte buffer using [`ReadBytes`] or
/// [`WriteBytes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BytesError {
    /// Byte buffer was too short to attempt this operation.
    ///
    /// Either you attempted to:
    /// * read `n` bytes from the buffer, but the buffer had less than `n` bytes
    ///   available to read
    /// * write `n` bytes to the buffer, but the buffer had less than `n` bytes
    ///   of space left for writing
    #[error("buffer too short")]
    BufferTooShort,
    /// Read a varint as [`u64`], but it was too large for the given operation.
    #[error("varint too large")]
    VarintTooLarge,
}

/// Result type with [`BytesError`] as an error type.
pub type Result<T> = std::result::Result<T, BytesError>;

/// Maximum number of bytes that a [`u64`] as a
/// [`VarInt`](integer_encoding::VarInt) can take up when encoded.
pub const VARINT_MAX_LEN: usize = 10;

/// Gives information on the [encoded](Encode) length of an instance of this
/// value.
pub trait EncodeLen {
    /// Gets the [encoded](Encode) length of this value in bytes.
    fn encode_len(&self) -> usize;
}

/// Type which can be [encoded](Encode) using a constant number of bytes.
///
/// [`EncodeLen`] is automatically implemented for all types which implement
/// this trait.
pub trait ConstEncodeLen {
    /// Number of bytes required to encode a value of this type.
    ///
    /// All values of this type must take the same number of bytes for encoding.
    const ENCODE_LEN: usize;
}

impl<T: ConstEncodeLen> EncodeLen for T {
    fn encode_len(&self) -> usize {
        Self::ENCODE_LEN
    }
}

/// Defines how to encode this value into a [`WriteBytes`].
pub trait Encode {
    /// Writes this value into a [`WriteBytes`].
    ///
    /// # Errors
    ///
    /// Errors if the buffer is not long enough to fit the extra bytes.
    fn encode(&self, buf: &mut impl WriteBytes) -> Result<()>;
}

/// Defines how to decode a value of this type from a [`ReadBytes`].
pub trait Decode: Sized {
    /// Reads the next value of this type from the buffer, and advances the
    /// cursor of the buffer.
    ///
    /// # Errors
    ///
    /// Errors if the buffer does not have enough bytes left to read.
    fn decode(buf: &mut impl ReadBytes) -> Result<Self>;
}

/// Provides fallible functions for reading bytes from a [`Bytes`] or
/// [`BytesMut`](bytes::BytesMut).
pub trait ReadBytes {
    /// Reads the next [`u8`] and advances the cursor of the buffer.
    ///
    /// # Errors
    ///
    /// Errors if the buffer does not have enough bytes left to read.
    fn read_u8(&mut self) -> Result<u8>;

    /// Reads the next [`u16`] and advances the cursor of the buffer.
    ///
    /// # Errors
    ///
    /// Errors if the buffer does not have enough bytes left to read.
    fn read_u16(&mut self) -> Result<u16>;

    /// Reads the next [`u32`] and advances the cursor of the buffer.
    ///
    /// # Errors
    ///
    /// Errors if the buffer does not have enough bytes left to read.
    fn read_u32(&mut self) -> Result<u32>;

    /// Reads the next [`u64`] and advances the cursor of the buffer.
    ///
    /// # Errors
    ///
    /// Errors if the buffer does not have enough bytes left to read.
    fn read_u64(&mut self) -> Result<u64>;

    /// Reads the next [`u64`] as a [`VarInt`](integer_encoding::VarInt) and
    /// advances the cursor of the buffer.
    ///
    /// # Errors
    ///
    /// Errors if the buffer does not have enough bytes left to read.
    fn read_varint(&mut self) -> Result<u64>;

    /// Reads the next [`u64`] as a [`VarInt`](integer_encoding::VarInt) and
    /// advances the cursor of the buffer, converting the result to a [`usize`].
    ///
    /// If the [`u64`] is too large, [`BytesError::VarintTooLarge`] is returned.
    /// # Errors
    ///
    /// Errors if the buffer does not have enough bytes left to read, or if the
    /// varint value is too large to fit in a [`usize`].
    fn read_varint_usize(&mut self) -> Result<usize> {
        usize::try_from(self.read_varint()?).map_err(|_| BytesError::VarintTooLarge)
    }

    /// Reads the next `len` bytes and creates a new [`Bytes`], creating a clone
    /// of the underlying [`Bytes`] object.
    ///
    /// The clone is cheap as it only increases a reference count.
    ///
    /// # Errors
    ///
    /// Errors if the buffer has less than `len` bytes left to read.
    fn read_slice(&mut self, len: usize) -> Result<Bytes>;

    /// Reads the next `T` and advances the cursor of the buffer.
    ///
    /// # Errors
    ///
    /// Errors if the buffer does not have enough bytes left to read.
    #[inline]
    fn read<T: Decode>(&mut self) -> Result<T>
    where
        Self: Sized,
    {
        T::decode(self)
    }
}

/// Provides fallible functions for writing bytes to a
/// [`BytesMut`](bytes::BytesMut).
pub trait WriteBytes {
    /// Writes a [`u8`] into the buffer and advances the cursor.
    ///
    /// # Errors
    ///
    /// Errors if the buffer is not long enough to fit the extra bytes.
    fn write_u8(&mut self, value: u8) -> Result<()>;

    /// Writes a [`u16`] into the buffer and advances the cursor.
    ///
    /// # Errors
    ///
    /// Errors if the buffer is not long enough to fit the extra bytes.
    fn write_u16(&mut self, value: u16) -> Result<()>;

    /// Writes a [`u32`] into the buffer and advances the cursor.
    ///
    /// # Errors
    ///
    /// Errors if the buffer is not long enough to fit the extra bytes.
    fn write_u32(&mut self, value: u32) -> Result<()>;

    /// Writes a [`u64`] into the buffer and advances the cursor.
    ///
    /// # Errors
    ///
    /// Errors if the buffer is not long enough to fit the extra bytes.
    fn write_u64(&mut self, value: u64) -> Result<()>;

    /// Writes a [`u64`] into the buffer as a
    /// [`VarInt`](integer_encoding::VarInt) and advances the cursor.
    ///
    /// This will take up a maximum of [`VARINT_MAX_LEN`] bytes.
    ///
    /// # Errors
    ///
    /// Errors if the buffer is not long enough to fit the extra bytes.
    fn write_varint(&mut self, value: u64) -> Result<()>;

    /// Writes a [`usize`] into the buffer as a
    /// [`VarInt`](integer_encoding::VarInt) and advances the cursor.
    ///
    /// This will take up a maximum of [`VARINT_MAX_LEN`] bytes.
    ///
    /// # Errors
    ///
    /// Errors if the buffer is not long enough to fit the extra bytes, or if
    /// the given value is too large to fit in a [`u64`].
    fn write_varint_usize(&mut self, value: usize) -> Result<()> {
        let value = u64::try_from(value).map_err(|_| BytesError::VarintTooLarge)?;
        self.write_varint(value)
    }

    /// Writes a slice of bytes into the buffer and advances the cursor.
    ///
    /// # Errors
    ///
    /// Errors if the buffer is not long enough to fit the extra bytes.
    fn write_slice(&mut self, src: &[u8]) -> Result<()>;

    /// Writes a `T` into the buffer and advances the cursor.
    ///
    /// # Errors
    ///
    /// Errors if the buffer is not long enough to fit the extra bytes.
    #[inline]
    fn write<T: Encode>(&mut self, value: &T) -> Result<()>
    where
        Self: Sized,
    {
        value.encode(self)
    }
}
