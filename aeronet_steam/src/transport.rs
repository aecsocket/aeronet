use std::fmt::Debug;

use aeronet::{LaneProtocol, TryAsBytes, TryFromBytes};
use derivative::Derivative;
use steamworks::{networking_sockets::InvalidHandle, SteamError};

/// Error that occurs while processing a Steam networking transport.
#[derive(Derivative, thiserror::Error)]
#[derivative(
    Debug(bound = "<P::Send as TryAsBytes>::Error: Debug, <P::Recv as TryFromBytes>::Error: Debug"),
    // TODO: `steamworks::InvalidHandle` should derive Clone
    // Clone(bound = "<P::Send as TryAsBytes>::Error: Clone, <P::Recv as TryFromBytes>::Error: Clone")
)]
pub enum SteamTransportError<P>
where
    P: LaneProtocol,
    P::Send: TryAsBytes,
    P::Recv: TryFromBytes,
{
    // internal
    /// Attempted to establish a new connection while the client was already
    /// connected to a server.
    #[error("already connected")]
    AlreadyConnected,
    /// Attempted to disconnect a client while it was already disconnected.
    #[error("already disconnected")]
    AlreadyDisconnected,
    /// Attempted to perform an action which requires a connection, while no
    /// connection is established.
    #[error("not connected")]
    NotConnected,

    // connect
    /// Failed to connect to the endpoint.
    #[error("failed to connect")]
    Connect(#[source] InvalidHandle),
    /// Failed to configure the lanes of the connection.
    #[error("failed to configure lanes")]
    ConfigureLanes(#[source] SteamError),

    // transport
    #[error("timed out")]
    TimedOut,
    #[error("failed to serialize message")]
    Serialize(#[source] <P::Send as TryAsBytes>::Error),
    #[error("failed to send message")]
    Send(#[source] SteamError),
    #[error("failed to deserialize message")]
    Deserialize(#[source] <P::Recv as TryFromBytes>::Error),
}
