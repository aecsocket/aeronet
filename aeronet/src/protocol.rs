use std::fmt::Display;

use crate::message::Message;

/// Defines what types of messages are transported between the client and the
/// server.
///
/// All transports take a `P` type parameter, which requires a type implementing
/// this trait. Different transports may put their own bounds on the type of
/// `P` if they need more information on how you use the transport. For example,
/// if messages in the transport are sent over a network, the transport may
/// require `P::C2S` or `P::S2C` implement [`TryIntoBytes`].
///
/// To create a protocol, define a unit struct and implement the desired types
/// on it:
///
/// ```
/// use aeronet::{message::Message, protocol::TransportProtocol};
///
/// #[derive(Debug, Message)]
/// struct MyMessage {/* ... */}
///
/// struct MyProtocol;
///
/// impl TransportProtocol for MyProtocol {
///     // client-to-server and server-to-client messages may be the same type
///     type C2S = MyMessage;
///     type S2C = MyMessage;
/// }
/// ```
///
/// The same protocol type should be used for both [`ClientTransport`] and
/// [`ServerTransport`]. However, the types of messages sent client-to-server
/// (C2S) and server-to-client (S2C) may be different.
///
/// [`ClientTransport`]: crate::client::ClientTransport
/// [`ServerTransport`]: crate::server::ServerTransport
/// [`TryIntoBytes`]: crate::message::TryIntoBytes
pub trait TransportProtocol: Send + Sync + 'static {
    /// Type of message sent from the client to the server.
    type C2S: Message;

    /// Type of message sent from the server to the client.
    type S2C: Message;
}

/// Unique version number of a transport protocol.
///
/// This may be used at the protocol level to ensure that two endpoints are
/// communicating on the same version of the same protocol.
///
/// This is treated as an opaque value, and is only used for equality
/// comparison. No ordering guarantees are made (i.e. `20` is not
/// necessarily a newer version than `10`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "bitcode", derive(bitcode::Encode, bitcode::Decode))]
pub struct ProtocolVersion(pub u64);

impl Display for ProtocolVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#018x}", self.0)
    }
}
