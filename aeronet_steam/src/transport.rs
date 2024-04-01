use std::{
    fmt::{Debug, Display},
    time::Duration,
};

use aeronet::{
    message::{TryFromBytes, TryIntoBytes},
    stats::{ByteStats, MessageStats, Rtt},
};
use aeronet_proto::{negotiate, packet};
use derivative::Derivative;
use steamworks::{
    networking_sockets::{NetConnection, NetworkingSockets},
    networking_types::NetConnectionEnd,
    SteamError,
};

pub use aeronet_proto::packet::PacketsConfig;

// use crate::server::ClientKey;

pub const MTU: usize = 512 * 1024;

/// Statistics on a Steamworks client/server connection.
#[derive(Debug, Clone, Default)]
pub struct ConnectionStats {
    /// See [`Rtt`].
    pub rtt: Duration,
    pub connection_quality_local: f32,
    pub connection_quality_remote: f32,
    pub out_packets_per_sec: f32,
    pub out_bytes_per_sec: f32,
    pub in_packets_per_sec: f32,
    pub in_bytes_per_sec: f32,
    pub send_rate_bytes_per_sec: u32,
    pub pending: u32,
    pub queued_send_bytes: u64,
}

impl ConnectionStats {
    #[must_use]
    pub fn from_connection<M: 'static>(
        socks: &NetworkingSockets<M>,
        conn: &NetConnection<M>,
    ) -> Self {
        let Ok((info, _)) = socks.get_realtime_connection_status(conn, 0) else {
            return Self::default();
        };

        Self {
            rtt: u64::try_from(info.ping())
                .map(Duration::from_millis)
                .unwrap_or_default(),
            connection_quality_local: info.connection_quality_local(),
            connection_quality_remote: info.connection_quality_remote(),
            out_packets_per_sec: info.out_packets_per_sec(),
            out_bytes_per_sec: info.out_bytes_per_sec(),
            in_packets_per_sec: info.in_packets_per_sec(),
            in_bytes_per_sec: info.in_bytes_per_sec(),
            send_rate_bytes_per_sec: u32::try_from(info.send_rate_bytes_per_sec())
                .unwrap_or_default(),
            pending: u32::try_from(info.pending_unreliable()).unwrap_or_default(),
            queued_send_bytes: u64::try_from(info.queued_send_bytes()).unwrap_or_default(),
        }
    }
}

impl Rtt for ConnectionStats {
    fn rtt(&self) -> Duration {
        self.rtt
    }
}

// TODO
#[derive(Debug, Clone)]
struct ClientKey;

impl Display for ClientKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
}

/// Error that occurs while processing a Steam networking transport.
#[derive(Derivative, thiserror::Error)]
#[derivative(
    Debug(bound = "S::Error: Debug, R::Error: Debug"),
    Clone(bound = "S::Error: Clone, R::Error: Clone")
)]
pub enum SteamTransportError<S: TryIntoBytes, R: TryFromBytes> {
    #[error("internal error")]
    InternalError,

    // client
    /// Attempted to disconnect the client while it was already disconnected.
    #[error("client already disconnected")]
    AlreadyDisconnected,
    /// Attempted to establish a new connection while the client was already
    /// connected to a server.
    #[error("client already connected")]
    AlreadyConnected,
    /// Attempted to perform an action which requires a connection, while no
    /// connection is established.
    #[error("client not connected")]
    NotConnected,
    /// Failed to start connecting the client to the given remote.
    #[error("client failed to start connecting")]
    StartConnecting,
    #[error("client connection rejected by server")]
    ConnectionRejected,
    #[error("connection lost")]
    ConnectionLost,

    // server
    /// Attempted to close the server while it was already closed.
    #[error("already closed")]
    AlreadyClosed,
    /// Attempted to open the server while it was already opening or open.
    #[error("already open")]
    AlreadyOpen,
    /// Attempted to perform an action which requires the server to be open
    /// while it is not.
    #[error("server not open")]
    NotOpen,
    /// Failed to create a listen socket to receive incoming connections on.
    #[error("failed to create listen socket")]
    CreateListenSocket,

    // server-side clients
    #[error("no client with key {client}")]
    NoClient { client: ClientKey },
    #[error("client {client} is already connected")]
    ClientAlreadyConnected { client: ClientKey },
    #[error("already responded to this session request")]
    AlreadyRespondedToRequest,

    // connect
    #[error("disconnected: {0:?}")]
    Disconnected(NetConnectionEnd),
    #[error("failed to send negotiation request")]
    SendNegotiateRequest(#[source] SteamError),
    #[error("failed to read negotiation request")]
    NegotiateRequest(#[source] negotiate::RequestError),
    #[error("failed to read negotiation response")]
    NegotiateResponse(#[source] negotiate::ResponseError),
    #[error("wrong protocol version")]
    WrongProtocolVersion(#[source] negotiate::WrongProtocolVersion),

    // transport
    #[error("failed to buffer message for sending")]
    BufferSend(#[source] packet::SendError<S>),
    #[error("failed to send message")]
    Send(#[source] SteamError),

    #[error("failed to receive message")]
    Recv2(#[source] packet::RecvError<R>),
    #[error("failed to receive messages")]
    Recv,
}
