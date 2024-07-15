mod backend;
mod frontend;

pub use frontend::*;

use std::{fmt::Debug, net::SocketAddr};

use aeronet::{
    lane::LaneKind,
    message::{TryFromBytes, TryIntoBytes},
    protocol::{ProtocolVersion, TransportProtocol},
};
use aeronet_proto::{negotiate, packet, seq::Seq};
use derivative::Derivative;
use steamworks::{SteamError, SteamId};

use crate::transport::MTU;

/// Identifier of a peer which a Steam client wants to connect to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectTarget {
    /// Peer identified by its IP address.
    Ip(SocketAddr),
    /// Peer identified by its Steam ID.
    Peer {
        /// Steam ID of the peer.
        steam_id: SteamId,
        /// Port to connect on.
        virtual_port: i32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientMessageKey {
    msg_seq: Seq,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamClientConfig {
    pub max_packet_len: usize,
    pub default_packet_cap: usize,
    pub max_sent_bytes_per_sec: usize,
    pub version: ProtocolVersion,
    pub lanes: Box<[LaneKind]>,
}

impl Default for SteamClientConfig {
    fn default() -> Self {
        Self {
            max_packet_len: MTU,
            default_packet_cap: 1024,
            max_sent_bytes_per_sec: 28_800_000, // TODO document
            version: ProtocolVersion::default(),
            lanes: Box::new([]),
        }
    }
}

impl SteamClientConfig {
    pub fn new(version: ProtocolVersion, lanes: &[LaneKind]) -> Self {
        Self {
            version,
            lanes: Box::from(lanes),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum BackendError {
    #[error("invalid handle")]
    InvalidHandle,
    #[error("frontend closed")]
    FrontendClosed,

    #[error("failed to create connection")]
    CreateConnection,
    #[error("connection rejected by peer")]
    Rejected,
    #[error("connection failed")]
    Failed,
    #[error("failed to send negotiation request")]
    SendNegotiate(#[source] SteamError),
    #[error("failed to negotiate protocol")]
    Negotiate(#[source] negotiate::ResponseError),

    #[error("failed to send packet")]
    Send(#[source] SteamError),
}

#[derive(Derivative, thiserror::Error)]
#[derivative(
    Debug(bound = "packet::SendError<P::C2S>: Debug, packet::RecvError<P::S2C>: Debug"),
    Clone(bound = "packet::SendError<P::C2S>: Clone, packet::RecvError<P::S2C>: Clone")
)]
pub enum SteamClientError<P>
where
    P: TransportProtocol,
    P::C2S: TryIntoBytes,
    P::S2C: TryFromBytes,
{
    #[error("already connected")]
    AlreadyConnected,
    #[error("already disconnected")]
    AlreadyDisconnected,
    #[error("not connected")]
    NotConnected,
    #[error("backend closed")]
    BackendClosed,

    #[error("failed to send message")]
    Send(#[source] packet::SendError<P::C2S>),
    #[error("failed to receive message")]
    Recv(#[source] packet::RecvError<P::S2C>),
    #[error(transparent)]
    Backend(#[from] BackendError),
}
