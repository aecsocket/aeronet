use std::error::Error;

/// Smallest unit of data which can be sent between transports.
/// 
/// A message is the smallest unit of transmission that transports use, as far
/// as the API is concerned. This trait defines a lower bound for what a message
/// must implement, however transports are free to add their own bounds onto
/// what kinds of messages they can send. For example, a networked transport may
/// require that messages can be serialized to/from a byte form - see
/// [`TryAsBytes`] and [`TryFromBytes`].
/// 
/// This trait can be derived - see [`aeronet_derive::Message`].
pub trait Message: Send + Sync + 'static {}

/// Attempt to convert this type into a slice of bytes.
/// 
/// Transports may require this as a bound on the outgoing message type, if the
/// message needs to be serialized into bytes first before being sent over e.g.
/// a network.
/// 
/// For the incoming counterpart, see [`TryFromBytes`].
pub trait TryAsBytes {
    /// Output type of [`TryAsBytes::try_as_bytes`], which can be dereferenced
    /// into a `[u8]`.
    type Output<'a>: AsRef<[u8]> + 'a
    where
        Self: 'a;

    /// Error type of [`TryAsBytes::try_as_bytes`].
    type Error: Error + Send + Sync + 'static;

    /// Attempts to convert this value into [`TryAsBytes::Output`].
    /// 
    /// # Errors
    /// 
    /// Errors if the conversion fails.
    fn try_as_bytes(&self) -> Result<Self::Output<'_>, Self::Error>;
}

/// Attempt to convert a sequence of bytes into a value of this type.
/// 
/// Transports may require this as a bound on the incoming message type, if the
/// message needs to be deserialized from a byte sequence after receiving data.
/// 
/// For the outgoing counterpart, see [`TryAsBytes`].
pub trait TryFromBytes {
    /// Error type of [`TryFromBytes::try_from_bytes`].
    type Error: Error + Send + Sync + 'static;

    /// Attempts to convert a sequence of bytes into a value of this type.
    /// 
    /// # Errors
    /// 
    /// Errors if the conversion fails.
    fn try_from_bytes(buf: &[u8]) -> Result<Self, Self::Error>
    where
        Self: Sized;
}
