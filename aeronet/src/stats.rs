//! Types for representing statistics of a transport, such as network latency
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

/// Holds statistics on the number of messages sent across a transport.
///
/// Implementors must ensure that, when increasing these counters, saturating
/// addition is used in order to avoid panics or overflows - see
/// [`usize::saturating_add`].
///
/// Note that a counter increasing does not necessarily mean that a message was
/// *successfully* sent or received - just that it was recognized.
pub trait MessageStats {
    /// Number of messages sent.
    ///
    /// For each unit in this value, a single `send` call was issued on the
    /// transport.
    fn msgs_sent(&self) -> usize;

    /// Number of messages received.
    ///
    /// For each unit in this value, a single `Recv` event was emitted by the
    /// transport during polling.
    fn msgs_recv(&self) -> usize;
}

/// Holds statistics on the number of message bytes sent across a transport.
///
/// This is used by transports which convert messages into a byte form, and
/// counts how many of these bytes have been sent or received. This specifically
/// excludes bytes used for frames or headers.
///
/// Note that a counter increasing does not necessarily mean that a message was
/// *successfully* sent or received - just that it was recognized.
///
/// Implementors must ensure that, when increasing these counters, saturating
/// addition is used in order to avoid panics or overflows - see
/// [`usize::saturating_add`].
pub trait MessageByteStats {
    /// Number of message bytes successfully sent.
    fn msg_bytes_sent(&self) -> usize;

    /// Number of message bytes successfully received.
    fn msg_bytes_recv(&self) -> usize;
}

/// Allows access to the local socket address of a connection.
///
/// Networked transports will use an operating system socket for network
/// communication, which has a specific address. This trait exposes this
/// socket's address.
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
