//! Types for representing statistics of a transport, such as network latency
//! and packet loss.
//!
//! Traits under this module may be implemented by your transport's
//! [`ClientTransport::Connected`] or [`ServerTransport::Connected`] types, in
//! which case you can use [`ClientTransport::state`] or
//! [`ServerTransport::client_state`] respectively to access it.
//!
//! [`ClientTransport::Connected`]: crate::client::ClientTransport::Connected
//! [`ServerTransport::Connected`]: crate::server::ServerTransport::Connected
//! [`ClientTransport::state`]: crate::client::ClientTransport::state
//! [`ServerTransport::client_state`]: crate::server::ServerTransport::client_state

use std::net::SocketAddr;

use web_time::Duration;

/// Gets the round-trip time (RTT) of a connection.
///
/// The RTT is defined as the time taken for the following to happen:
/// * a message is sent
/// * the other endpoint receives it
/// * the other endpoint processes the message
/// * a response message is received
///
/// This will never give the exact RTT value, as it is constantly in flux as
/// network conditions change. However, it aims to be a good-enough estimate for
/// use in e.g. lag compensation estimates, or displaying to other clients.
///
/// See [`stats`](crate::stats) on how to get access to this info.
#[doc(alias = "latency")]
#[doc(alias = "ping")]
pub trait Rtt {
    /// Gets the round-trip time.
    fn rtt(&self) -> Duration;
}

/// Holds statistics on the number of bytes sent across a transport.
///
/// Note that a counter increasing does not necessarily mean that a message was
/// *successfully* sent or received:
/// - for sending, it indicates how many bytes we attempted to send
/// - for receiving, it indicates how many bytes we received and acknowledged
///
/// See [`stats`](crate::stats) on how to get access to this info.
///
/// Implementors must ensure that, when increasing these counters, saturating
/// addition is used in order to avoid panics or overflows - see
/// [`usize::saturating_add`].
pub trait MessageStats {
    /// Gets the number of message bytes successfully sent.
    fn bytes_sent(&self) -> usize;

    /// Gets the number of message bytes successfully received.
    fn bytes_recv(&self) -> usize;
}

/// Allows access to the local socket address of a connection.
///
/// Networked transports will use an operating system socket for network
/// communication, which has a specific address. This trait exposes the address
/// of our side's socket.
///
/// See [`stats`](crate::stats) on how to get access to this info.
///
/// To access the remote address of a connection, see [`RemoteAddr`].
pub trait LocalAddr {
    /// Gets the local socket address of a connection.
    fn local_addr(&self) -> SocketAddr;
}

/// Allows access to the remote socket address of a connection.
///
/// Networked transports will use an operating system socket for network
/// communication, which has a specific address. This trait exposes the address
/// of the other side of the socket.
///
/// See [`stats`](crate::stats) on how to get access to this info.
///
/// To access the local address of a connection, see [`LocalAddr`].
pub trait RemoteAddr {
    /// Gets the remote socket address of a connection.
    fn remote_addr(&self) -> SocketAddr;
}
