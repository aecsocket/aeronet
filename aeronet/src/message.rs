pub use aeronet_derive::Message;

use std::{convert::Infallible, error::Error};

use bytes::Bytes;

/// Smallest unit of data which can be sent between transports.
///
/// A message is the smallest unit of transmission that transports use, as far
/// as the API is concerned. This trait defines a lower bound for what a message
/// must implement, however transports are free to add their own bounds onto
/// what kinds of messages they can send. For example, a networked transport may
/// require that messages can be serialized to/from a byte form - see
/// [`TryIntoBytes`] and [`TryFromBytes`].
///
/// This trait can be derived - see [`aeronet_derive::Message`].
pub trait Message: Send + Sync + 'static {}

impl Message for () {}

/// Attempt to convert this type into [`Bytes`].
///
/// Transports may require this as a bound on the outgoing message type, if the
/// message needs to be serialized into bytes first before being sent over e.g.
/// a network.
///
/// For the incoming counterpart, see [`TryFromBytes`].
pub trait TryIntoBytes {
    /// Error type of [`TryIntoBytes::try_into_bytes`].
    type Error: Error + Send + Sync + 'static;

    /// Attempts to convert this value into [`Bytes`].
    ///
    /// # Errors
    ///
    /// Errors if the conversion fails.
    fn try_into_bytes(self) -> Result<Bytes, Self::Error>;
}

impl TryIntoBytes for () {
    type Error = Infallible;

    fn try_into_bytes(self) -> Result<Bytes, Self::Error> {
        Ok(Bytes::new())
    }
}

impl TryIntoBytes for Bytes {
    type Error = Infallible;

    fn try_into_bytes(self) -> Result<Bytes, Self::Error> {
        Ok(self)
    }
}

impl TryIntoBytes for Vec<u8> {
    type Error = Infallible;

    fn try_into_bytes(self) -> Result<Bytes, Self::Error> {
        Ok(Bytes::from(self))
    }
}

/// Attempt to convert [`Bytes`] into a value of this type.
///
/// Transports may require this as a bound on the incoming message type, if the
/// message needs to be deserialized from a byte sequence after receiving data.
///
/// For the outgoing counterpart, see [`TryIntoBytes`].
pub trait TryFromBytes {
    /// Error type of [`TryFromBytes::try_from_bytes`].
    type Error: Error + Send + Sync + 'static;

    /// Attempts to convert a sequence of bytes into a value of this type.
    ///
    /// # Errors
    ///
    /// Errors if the conversion fails.
    fn try_from_bytes(buf: Bytes) -> Result<Self, Self::Error>
    where
        Self: Sized;
}

impl TryFromBytes for () {
    type Error = Infallible;

    fn try_from_bytes(_: Bytes) -> Result<Self, Self::Error> {
        Ok(())
    }
}

impl TryFromBytes for Bytes {
    type Error = Infallible;

    fn try_from_bytes(buf: Bytes) -> Result<Self, Self::Error> {
        Ok(buf)
    }
}

impl TryFromBytes for Vec<u8> {
    type Error = Infallible;

    fn try_from_bytes(buf: Bytes) -> Result<Self, Self::Error> {
        Ok(buf.to_vec())
    }
}
