use bytes::Bytes;

pub mod chunks;
pub mod impl_bytes;
pub mod varint;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BytesError {
    /// Byte buffer was too short to attempt this operation.
    ///
    /// Either you attempted to:
    /// * read `n` bytes from the buffer, but the buffer had less than `n` bytes
    ///   available to read
    /// * write `n` bytes to the buffer, but the buffer had less than `n` bytes
    ///   of capacity left for writing
    #[error("buffer too short")]
    BufferTooShort,
    /// Varint was too large to encode/decode.
    #[error("varint too large")]
    VarInt,
}

pub type Result<T> = std::result::Result<T, BytesError>;

pub trait ReadBytes {
    fn read_u8(&mut self) -> Result<u8>;

    fn read_u16(&mut self) -> Result<u16>;

    fn read_u32(&mut self) -> Result<u32>;

    fn read_u64(&mut self) -> Result<u64>;

    fn read_varint(&mut self) -> Result<u64>;

    fn read_slice(&mut self, len: usize) -> Result<Bytes>;
}

pub trait WriteBytes {
    fn write_u8(&mut self, value: u8) -> Result<()>;

    fn write_u16(&mut self, value: u16) -> Result<()>;

    fn write_u32(&mut self, value: u32) -> Result<()>;

    fn write_u64(&mut self, value: u64) -> Result<()>;

    fn write_varint(&mut self, value: u64) -> Result<()>;

    fn write_slice(&mut self, src: &[u8]) -> Result<()>;
}
