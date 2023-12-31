use std::{fmt::Debug, net::SocketAddr, time::Duration};

use crate::{protocol::TimeoutConfig, LaneKey, Message};

pub trait TransportProtocol: Send + Sync + 'static {
    type C2S: Message;

    type S2C: Message;
}

pub trait LaneProtocol: TransportProtocol {
    type Lane: LaneKey;
}

#[derive(Debug, Clone, Default)]
pub struct TransportConfig {
    pub timeout: TimeoutConfig,
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
pub trait MessageStats {
    /// Total number of messages sent.
    fn msgs_sent(&self) -> usize;

    /// Total number of messages received.
    fn msgs_recv(&self) -> usize;
}

/// Holds statistics on the bytes sent across a transport.
///
/// This is used by transports which convert messages into a byte form.
pub trait ByteStats {
    /// Total number of message bytes sent.
    ///
    /// This only counts the bytes that make up a message, rather than all
    /// bytes including transport-layer wrappers and frames.
    fn bytes_sent(&self) -> usize;

    /// Total number of bytes received.
    ///
    /// This only counts the bytes that make up a message, rather than all
    /// bytes including transport-layer wrappers and frames.
    fn bytes_recv(&self) -> usize;
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
