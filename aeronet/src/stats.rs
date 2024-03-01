//! Items for representing statistics of a transport, such as network latency
//! and packet loss.

use std::{net::SocketAddr, time::Duration};

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
