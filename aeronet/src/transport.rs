use std::{net::SocketAddr, time::Duration};

use crate::{ChannelKey, Message};

/// Defines the types of messages sent across a transport channel.
///
/// You should define one type that implements this trait in a single central
/// place in your app, and use it as the protocol type parameter on transport
/// implementations.
///
/// ```
/// use aeronet::{Message, TransportProtocol};
///
/// struct AppProtocol;
///
/// #[derive(Message)]
/// struct AppMessage {/* ... */}
///
/// impl TransportProtocol for AppProtocol {
///     type C2S = AppMessage;
///     type S2C = AppMessage;
/// }
/// ```
pub trait TransportProtocol: Send + Sync + 'static {
    /// The type of message sent from the client to the server.
    type C2S: Message;

    /// The type of message sent from the server to the client.
    type S2C: Message;
}

/// Extension of [`TransportProtocol`] which specifies along which
/// [`ChannelKey`] messages are sent.
pub trait ChannelProtocol: TransportProtocol {
    /// The type of [`ChannelKey`] used to specify along what channel a message
    /// is sent.
    type Channel: ChannelKey;
}

/// Allows access to the round-trip time of a connection.
///
/// This is also known as latency, or "ping".
#[doc(alias = "ping")]
#[doc(alias = "latency")]
pub trait Rtt {
    /// Gets the round-trip time to the connected endpoint.
    ///
    /// The round-trip time is defined as the time taken for the following to
    /// happen:
    /// * client sends data
    /// * server receives the data and sends a response
    ///   * the processing time is assumed to be instant
    /// * client receives data
    fn rtt(&self) -> Duration;
}

/// Allows access to the local socket address of a connection.
///
/// If a connection uses an operating system socket for communication with the
/// other side, this returns the address of that socket.
pub trait LocalAddr {
    /// Gets the local socket address of this endpoint.
    fn local_addr(&self) -> SocketAddr;
}

/// Allows access to the remote socket address of the other side of a
/// connection.
///
/// If a connection uses an operating system socket for communication with the
/// other side, this returns the address of that socket.
pub trait RemoteAddr {
    /// Gets the remote socket address of the endpoint that this side is
    /// connected to.
    fn remote_addr(&self) -> SocketAddr;
}
