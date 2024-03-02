use bytes::Bytes;
use bytes_varint::{VarIntError, VarIntSupport};
use safer_bytes::error::Truncated;

/// Error for reading from [`Bytes`].
///
/// Provides a wrapper over [`safer_bytes::error`] and
/// [`bytes_varint::VarIntError`] error types.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ReadError {
    /// See [`Truncated`].
    #[error("buffer too short")]
    TooShort,
    /// See [`VarIntError`].
    #[error("failed to read varint")]
    ReadVarInt,
}

impl From<Truncated> for ReadError {
    fn from(_: Truncated) -> Self {
        Self::TooShort
    }
}

impl From<VarIntError> for ReadError {
    fn from(_: VarIntError) -> Self {
        Self::ReadVarInt
    }
}

/// Extension trait on [`Bytes`].
pub trait TryGetVarintExt {
    /// Attempts to read the next u64 varint, and advances the cursor.
    ///
    /// # Errors
    ///
    /// See [`VarIntError`].
    fn try_get_varint(&mut self) -> Result<u64, ReadError>;
}

impl TryGetVarintExt for Bytes {
    fn try_get_varint(&mut self) -> Result<u64, ReadError> {
        self.get_u64_varint().map_err(|_| ReadError::ReadVarInt)
    }
}
