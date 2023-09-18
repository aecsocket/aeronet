//! Traits describing data that can be sent or received by a transport.
//!
//! A message is some data that is sent or received by a specific side of the connection. This
//! module provides the traits:
//! * [`SendMessage`] for messages which are sent by this side
//! * [`RecvMessage`] for messages which are received by this side
//! 
//! The transports may wish to transport the messages as a byte sequence instead.

use anyhow::Result;

/// Data that can be sent from the current side to the opposite side.
///
/// See [the module docs](self) for more info.
pub trait SendMessage: Send + Sync + Clone + 'static {
    /// Converts this message into its payload form as bytes.
    fn into_payload(self) -> Result<Vec<u8>>;
}

/// Data that can be received from the opposite side by the current side.
///
/// See [the module docs](self) for more info.
pub trait RecvMessage: Send + Sync + Sized + 'static {
    /// Converts a payload form in a byte buffer into this message.
    fn from_payload(buf: &[u8]) -> Result<Self>;
}
