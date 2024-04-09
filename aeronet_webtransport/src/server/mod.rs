mod backend;
mod frontend;

use std::fmt::{Debug, Display};

use aeronet::{lane::LaneKind, message::BytesMapper, protocol::ProtocolVersion};
use aeronet_proto::packet::{self, LaneConfig};
use derivative::Derivative;
pub use frontend::*;
use wtransport::error::ConnectionError;

use crate::shared::{self, WebTransportProtocol};

slotmap::new_key_type! {
    pub struct ClientKey;
}

impl Display for ClientKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

pub type NativeConfig = wtransport::ServerConfig;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ServerConfig {
    pub version: ProtocolVersion,
    pub lanes_recv: Vec<LaneKind>,
    pub lanes_send: Vec<LaneConfig>,
    pub total_bandwidth: usize,
    pub client_bandwidth: usize,
    pub max_packet_len: usize,
    pub default_packet_cap: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            version: ProtocolVersion::default(),
            lanes_recv: Vec::new(),
            lanes_send: Vec::new(),
            total_bandwidth: shared::DEFAULT_BANDWIDTH,
            client_bandwidth: shared::DEFAULT_BANDWIDTH,
            max_packet_len: shared::DEFAULT_MTU,
            default_packet_cap: shared::DEFAULT_MTU,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectionResponse {
    Accept,
    Forbidden,
    NotFound,
}

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("failed to await session request")]
    AwaitSessionRequest(#[source] ConnectionError),
    #[error("failed to accept session request")]
    AcceptSessionRequest(#[source] ConnectionError),
    #[error("server forced disconnect")]
    ForceDisconnect,
    #[error(transparent)]
    Generic(#[from] shared::BackendError),
}

#[derive(Derivative, thiserror::Error)]
#[derivative(Debug(
    bound = "packet::SendError<<P::Mapper as BytesMapper<P::S2C>>::IntoError>: Debug, packet::RecvError<<P::Mapper as BytesMapper<P::C2S>>::FromError>: Debug"
))]
pub enum ServerError<P: WebTransportProtocol> {
    #[error("already open")]
    AlreadyOpen,
    #[error("already closed")]
    AlreadyClosed,
    #[error("not open")]
    NotOpen,
    #[error("no client with key {client_key}")]
    NoClient { client_key: ClientKey },
    #[error("client {client_key} not requesting connection")]
    ClientNotRequesting { client_key: ClientKey },
    #[error("already responded to client {client_key}'s connection request")]
    AlreadyResponded { client_key: ClientKey },
    #[error("client {client_key} not connected")]
    ClientNotConnected { client_key: ClientKey },
    #[error("backend closed")]
    BackendClosed,
    #[error("client backend closed")]
    ClientBackendClosed,

    #[error(transparent)]
    Backend(#[from] BackendError),
    #[error(transparent)]
    Send(#[from] packet::SendError<<P::Mapper as BytesMapper<P::S2C>>::IntoError>),
    #[error(transparent)]
    Recv(#[from] packet::RecvError<<P::Mapper as BytesMapper<P::C2S>>::FromError>),
}
