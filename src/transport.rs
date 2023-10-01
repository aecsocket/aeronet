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
    #[error("side closed")]
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
