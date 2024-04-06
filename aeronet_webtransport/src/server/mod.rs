mod backend;
mod frontend;

use std::fmt::{Debug, Display};

use aeronet::{lane::LaneKind, message::BytesMapper, protocol::ProtocolVersion};
use aeronet_proto::packet;
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

#[derive(Derivative)]
#[derivative(Debug(bound = "P::Mapper: Debug"))]
pub struct ServerConfig<P: WebTransportProtocol> {
    #[derivative(Debug = "ignore")]
    pub native: NativeConfig,
    pub version: ProtocolVersion,
    pub lanes_in: Box<[LaneKind]>,
    pub lanes_out: Box<[LaneKind]>,
    pub mapper: P::Mapper,
    pub total_bandwidth: usize,
    pub client_bandwidth: usize,
    pub max_packet_len: usize,
    pub default_packet_cap: usize,
}

impl<P: WebTransportProtocol> ServerConfig<P> {
    pub fn new(native: impl Into<wtransport::ServerConfig>, mapper: P::Mapper) -> Self {
        Self {
            native: native.into(),
            version: ProtocolVersion::default(),
            lanes_in: Box::default(),
            lanes_out: Box::default(),
            mapper,
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
pub enum ServerBackendError {
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
    Backend(#[from] ServerBackendError),
    #[error(transparent)]
    Send(#[from] packet::SendError<<P::Mapper as BytesMapper<P::S2C>>::IntoError>),
    #[error(transparent)]
    Recv(#[from] packet::RecvError<<P::Mapper as BytesMapper<P::C2S>>::FromError>),
}
