use std::fmt;

use bitcode::{Decode, Encode};

use crate::{LaneKey, Message};

/// Defines what types of messages are transported between the client and the
/// server.
///
/// All transports take a `P` type parameter, which requires a type implementing
/// this trait. Different transports may put their own bounds on the type of
/// `P` if they need more information on how you use the transport. For example,
/// if messages in the transport are sent over different [lanes](LaneKey), the
/// protocol must implement [`LaneProtocol`].
///
/// To create a protocol, define a unit struct and implement the desired types
/// on it:
///
/// ```
/// use aeronet::{Message, TransportProtocol};
///
/// #[derive(Debug, Message)]
/// struct MyMessage { /* ... */ }
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
/// [`ClientTransport`]: crate::ClientTransport
/// [`ServerTransport`]: crate::ServerTransport
pub trait TransportProtocol: Send + Sync + 'static {
    /// Type of message sent from the client to the server.
    type C2S: Message;

    /// Type of message sent from the server to the client.
    type S2C: Message;
}

/// Defines what type of [`LaneKey`] that [`Message`]s are sent over.
///
/// Transports may send messages on different [lanes](LaneKey), and need a way
/// to determine:
/// * What are all of the possible lanes available to send messages on?
///   * For example, if a transport needs to set up lanes in advance, it needs
///     to know all of the possible lanes beforehand.
/// * What specific lane is this specific message sent on?
///
/// This trait allows the user to specify which user-defined type, implementing
/// [`LaneKey`], is used for these functions.
pub trait LaneProtocol: TransportProtocol {
    /// User-defined type of lane that the transport uses.
    type Lane: LaneKey;
}

///
/// This is treated as an opaque value, and is only used for equality
/// comparison. No ordering guarantees are made (i.e. `20` is not
/// necessarily a newer version than `10`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Encode, Decode)]
pub struct ProtocolVersion(pub u32);

impl fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#010x}", self.0)
    }
}

impl<T: VersionedProtocol> From<T> for ProtocolVersion {
    fn from(_: T) -> Self {
        T::VERSION
    }
}

/// Defines a version number of a protocol.
///
/// Before finalizing a connection, a transport may wish to assert that it is
/// talking to an endpoint which is using the same version of the same protocol.
/// This allows you to define a version number unique for your app and build,
/// which both sides of the connection compare before finalizing connection.
///
/// This number may be a constant which is manually updated by the developer
/// when a breaking protocol change occurs, or may be auto-generated at compile
/// time using i.e. the source files as inputs to a hash function.
pub trait VersionedProtocol: TransportProtocol {
    /// Version number of this protocol.
    const VERSION: ProtocolVersion;
}
