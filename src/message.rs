//! Traits describing data that can be sent or received by a transport.
//!
//! A message is some data that is sent or received by a specific side of the connection. This
//! module provides the traits:
//! * [`SendMessage`] for messages which are sent by this side
//! * [`RecvMessage`] for messages which are received by this side
//!
//! The transports may wish to transport the messages as a byte sequence. A message can be
//! converted into this *payload* form using [`SendMessage::into_payload`] and converted back
//! into its corresponding message using [`RecvMessage::from_payload`].

use anyhow::Result;

/// Data that can be sent from the current side to the opposite side.
///
/// The transport implementation may wish to transport the messages as a byte sequence.
/// This trait means the message can be converted into this *payload* form, able to be
/// sent over the wire.
///
/// See [`RecvMessage`] for the receiving counterpart.
///
/// # Serialization support
///
/// With the `bincode` feature enabled, this trait will automatically be implemented for types
/// which implement `serde::Serialize`.
pub trait SendMessage: Send + Sync + Clone + 'static {
    /// Attempts to convert this message into its payload form as bytes.
    fn into_payload(self) -> Result<Vec<u8>>;
}

/// Data that can be received from the opposite side by the current side.s
///
/// The transport implementation may wish to receive messages as a byte sequence.
/// This trait means the message can be deserialized from this *payload* form, able to be
/// received over the wire.
///
/// See [`SendMessage`] for the sending counterpart.
///
/// # Serialization support
///
/// With the `bincode` feature enabled, this trait will automatically be implemented for types
/// which implement `serde::de::DeserializeOwned`.
pub trait RecvMessage: Send + Sync + Sized + 'static {
    /// Attempts to convert a payload from a byte buffer into this message.
    fn from_payload(buf: &[u8]) -> Result<Self>;
}

#[cfg(feature = "bincode")]
impl<T> SendMessage for T
where
    T: Send + Sync + Clone + serde::Serialize + 'static,
{
    fn into_payload(self) -> Result<Vec<u8>> {
        bincode::serialize(&self).map_err(|err| err.into())
    }
}

#[cfg(feature = "bincode")]
impl<T> RecvMessage for T
where
    T: Send + Sync + Sized + serde::de::DeserializeOwned + 'static,
{
    fn from_payload(buf: &[u8]) -> Result<Self> {
        bincode::deserialize(buf).map_err(|err| err.into())
    }
}
