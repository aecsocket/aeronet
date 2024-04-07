//! Types of values which can be sent to/from a client/server using a transport.

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
/// Transports may also choose to use [`BytesMapper`] for converting values into
/// bytes. The implementation of [`BytesMapper`] for `()` will use this trait
/// for this conversion. If you need extra context for converting a value into
/// bytes, which can't exist in the value itself, look into [`BytesMapper`].
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
/// Transports may also choose to use [`BytesMapper`] for creating values from
/// bytes. The implementation of [`BytesMapper`] for `()` will use this trait
/// for this conversion. If you need extra context for creating a value from
/// bytes, which can't exist in the value itself, look into [`BytesMapper`].
///
/// For the outgoing counterpart, see [`TryIntoBytes`].
pub trait TryFromBytes: Sized {
    /// Error type of [`TryFromBytes::try_from_bytes`].
    type Error: Error + Send + Sync + 'static;

    /// Attempts to convert a sequence of bytes into a value of this type.
    ///
    /// # Errors
    ///
    /// Errors if the conversion fails.
    fn try_from_bytes(buf: Bytes) -> Result<Self, Self::Error>;
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

/// Allows converting messages into, and creating messages from, bytes.
///
/// Transports may include a value implementing this trait as a field, and use
/// it to map their messages to/from bytes.
///
/// # How do I make one?
///
/// If your message type already implements [`TryIntoBytes`] and
/// [`TryFromBytes`], you don't need to make your own. Just use `()` as the
/// mapper value - it implements this trait.
///
/// # Why use this over [`TryIntoBytes`] or [`TryFromBytes`]?
///
/// In some cases, you may not have all the context you need in the message
/// itself in order to be able to convert it into bytes. You can instead store
/// this state in a type implementing this trait. When the transport attempts to
/// convert a message to bytes, it will call this value's function, letting you
/// use your existing context for the conversion.
pub trait BytesMapper<T> {
    /// Error type of [`BytesMapper::try_into_bytes`].
    type IntoError: Error + Send + Sync + 'static;

    /// Error type of [`BytesMapper::try_from_bytes`].
    type FromError: Error + Send + Sync + 'static;

    /// Attempts to convert a `T` into [`Bytes`].
    ///
    /// # Errors
    ///
    /// Errors if the conversion fails.
    fn try_into_bytes(&mut self, value: T) -> Result<Bytes, Self::IntoError>;

    /// Attempts to convert a sequence of bytes into a `T`.
    ///
    /// # Errors
    ///
    /// Errors if the conversion fails.
    fn try_from_bytes(&mut self, buf: Bytes) -> Result<T, Self::FromError>;
}

impl<T: TryIntoBytes + TryFromBytes> BytesMapper<T> for () {
    type IntoError = <T as TryIntoBytes>::Error;
    type FromError = <T as TryFromBytes>::Error;

    fn try_into_bytes(&mut self, value: T) -> Result<Bytes, Self::IntoError> {
        value.try_into_bytes()
    }

    fn try_from_bytes(&mut self, buf: Bytes) -> Result<T, Self::FromError> {
        T::try_from_bytes(buf)
    }
}
