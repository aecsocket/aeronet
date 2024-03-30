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
}

/// Result type with [`BytesError`] as an error type.
pub type Result<T> = std::result::Result<T, BytesError>;

/// Maximum number of bytes that a [`u64`] as a
/// [`VarInt`](integer_encoding::VarInt) can take up when encoded.
pub const VARINT_MAX_SIZE: usize = 10;

/// Gives information on the [encoded](Encode) size of an instance of this
/// value.
pub trait EncodeSize {
    /// Gets the [encoded](Encode) size of this value in bytes.
    fn encode_size(&self) -> usize;
}

/// Type which can be [encoded](Encode) using a constant number of bytes.
///
/// [`EncodeSize`] is automatically implemented for all types which implement
/// this trait.
pub trait ConstEncodeSize {
    /// Number of bytes required to encode a value of this type.
    ///
    /// All values of this type must take the same number of bytes for encoding.
    const ENCODE_SIZE: usize;
}

impl<T: ConstEncodeSize> EncodeSize for T {
    fn encode_size(&self) -> usize {
        Self::ENCODE_SIZE
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
    /// This will take up a maximum of [`VARINT_MAX_SIZE`] bytes.
    ///
    /// # Errors
    ///
    /// Errors if the buffer is not long enough to fit the extra bytes.
    fn write_varint(&mut self, value: u64) -> Result<()>;

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
