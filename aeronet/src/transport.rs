use std::{net::SocketAddr, time::Duration};

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

/// Gets the round-trip time of a connection.
///
/// The RTT is defined as the time taken for the following to happen:
/// * a message is sent
/// * the other endpoint receives it
/// * the other endpoint processes the message
/// * a reponse message is received
///
/// This will never give the exact RTT value, as it is constantly in flux as
/// network conditions change. However, it aims to be a good-enough estimate for
/// use in e.g. lag compensation estimates, or displaying to other clients.
#[doc(alias = "latency")]
#[doc(alias = "ping")]
pub trait Rtt {
    /// The round-trip time.
    fn rtt(&self) -> Duration;
}

/// Holds statistics on the messages sent across a transport.
///
/// In this context, "successful" means that the message was sent out along the
/// underlying transport mechanism, and the receiver should have been able to
/// receive the message, if there were no undetectable transport errors. If a
/// message was sent but not acknowledged, it still counts as a successful send.
pub trait MessageStats {
    /// Number of messages successfully sent.
    fn msgs_sent(&self) -> usize;

    /// Number of messages successfully received.
    fn msgs_recv(&self) -> usize;
}

/// Holds statistics on the bytes sent across a transport.
///
/// This is used by transports which convert messages into a byte form.
///
/// In this context, "successful" means that the message was sent out along the
/// underlying transport mechanism, and the receiver should have been able to
/// receive the message, if there were no undetectable transport errors. If a
/// message was sent but not acknowledged, it still counts as a successful send.
pub trait ByteStats {
    /// Number of message bytes successfully sent.
    ///
    /// This only counts the bytes which make up a message payload, and excludes
    /// any metadata e.g. packet frames and headers.
    fn msg_bytes_sent(&self) -> usize;

    /// Number of message bytes successfully received.
    ///
    /// This only counts the bytes which make up a message payload, and excludes
    /// any metadata e.g. packet frames and headers.
    fn msg_bytes_recv(&self) -> usize;

    /// Number of total bytes successfully sent.
    ///
    /// This counts all bytes sent along the transport (or at least as many as
    /// can be tracked), including packet frames and headers.
    fn total_bytes_sent(&self) -> usize;

    /// Number of total bytes successfully received.
    ///
    /// This counts all bytes sent along the transport (or at least as many as
    /// can be tracked), including packet frames and headers.
    fn total_bytes_recv(&self) -> usize;
}

/// Allows access to the local socket address of a connection.
///
/// Networked transports will use an operating system socket for network
/// communication, which has a specific address. This trait exposes this info
/// to users.
///
/// To access the remote address of a connection, see [`RemoteAddr`].
pub trait LocalAddr {
    /// The local socket address of a connection.
    fn local_addr(&self) -> SocketAddr;
}

/// Allows access to the remote socket address of a connection.
///
/// Networked transports will use an operating system socket for network
/// communication, which has a specific address. This trait exposes the socket
/// address of the side which this app's transport is connected to.
///
/// To access the local address of a connection, see [`LocalAddr`].
pub trait RemoteAddr {
    /// The remote socket address of a connection.
    fn remote_addr(&self) -> SocketAddr;
}
