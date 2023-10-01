//! Types used by the [`crate::ClientTransport`] and [`crate::ServerTransport`] types.

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

/// The reason why a client was disconnected from a server.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// The server was closed and all open client connections have been dropped.
    #[error("server closed")]
    ServerClosed,
    /// The server forced this client to disconnect.
    #[error("forced disconnect by server")]
    ForceDisconnect,
    /// The client failed to establish a connection to the server.
    #[error("failed to connect to server")]
    Connecting(#[source] anyhow::Error),
    /// There was an error in transport (receiving or sending data).
    #[error("transport error")]
    Transport(#[source] anyhow::Error),
}
