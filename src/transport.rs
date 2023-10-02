use std::{net::SocketAddr, time::Duration};

/// An error that occurrs while receiving queued events from a transport.
#[derive(Debug, thiserror::Error)]
pub enum RecvError {
    /// There are no more events to receive, however more events may be sent in the future.
    #[error("no events to receive")]
    Empty,
    /// The transport is closed and no more events will ever be received.
    #[error("transport closed")]
    Closed,
}

/// The reason why this side disconnected from the other side.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// This side was closed and all open connections have been dropped.
    #[error("transport closed")]
    Closed,
    /// This side forced a disconnect from the other side.
    #[error("forced disconnect")]
    ForceDisconnect,
    /// This side failed to establish a connection to the other side.
    #[error("failed to connect")]
    Connecting(#[source] anyhow::Error),
    /// There was an error in transport (receiving or sending data).
    #[error("transport error")]
    Transport(#[source] anyhow::Error),
}

/// Allows access to the round-trip time of a connection.
pub trait Rtt {
    /// Gets the round-trip time to the connected endpoint.
    ///
    /// The round-trip time is defined as the time taken for the following to happen:
    /// * client sends data
    /// * server receives the data and sends a response
    ///   * the processing time is assumed to be instant
    /// * client receives data
    fn rtt(&self) -> Duration;
}

/// Allows access to the remote socket address of the other side of a connection.
pub trait RemoteAddr {
    /// Gets the remote socket address of the endpoint that this side is connected to.
    fn remote_addr(&self) -> SocketAddr;
}
