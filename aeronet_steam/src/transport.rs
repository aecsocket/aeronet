use std::fmt::Debug;

use aeronet::{LaneKey, TryAsBytes, TryFromBytes};
use derivative::Derivative;
use steamworks::SteamError;

pub(super) const HANDSHAKE_CHANNEL: u32 = u32::MAX;
pub(super) const RECV_BATCH_SIZE: usize = 16;
pub(super) const CHALLENGE_SIZE: usize = 32;
pub(super) const DISCONNECT_TOKEN: &[u8] = "disconnect".as_bytes();

/// Error that occurs while processing a Steam networking transport.
#[derive(Derivative, thiserror::Error)]
#[derivative(
    Debug(bound = "S::Error: Debug, R::Error: Debug"),
    Clone(bound = "S::Error: Clone, R::Error: Clone")
)]
pub enum SteamTransportError<S, R, L>
where
    S: TryAsBytes,
    R: TryFromBytes,
    L: LaneKey,
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

    // handshaking
    /// Failed to send the initial connection request message to the other side.
    #[error("failed to send connect request")]
    SendConnectRequest(#[source] SteamError),
    /// The other side sent some data to our side to respond to our connection
    /// request, but
    #[error("received invalid handshake token")]
    InvalidHandshakeToken,
    #[error("disconnected by other side")]
    DisconnectedByOtherSide,

    // transport
    #[error("timed out")]
    TimedOut,
    #[error("on {lane:?}")]
    OnLane {
        lane: L,
        #[source]
        source: LaneError<S, R>,
    },
}

#[derive(Derivative, thiserror::Error)]
#[derivative(
    Debug(bound = "S::Error: Debug, R::Error: Debug"),
    Clone(bound = "S::Error: Clone, R::Error: Clone")
)]
pub enum LaneError<S, R>
where
    S: TryAsBytes,
    R: TryFromBytes,
{
    #[error("failed to serialize message")]
    Serialize(#[source] S::Error),
    #[error("failed to send message")]
    Send(#[source] SteamError),
    #[error("failed to deserialize message")]
    Deserialize(#[source] R::Error),
}
