mod backend;
mod frontend;

use std::fmt::{Debug, Display};

use aeronet::{
    lane::LaneKind,
    message::{TryFromBytes, TryIntoBytes},
    protocol::{ProtocolVersion, TransportProtocol},
};
use aeronet_proto::packet;
use derivative::Derivative;
pub use frontend::*;
use wtransport::error::ConnectionError;

use crate::error::BackendError;

slotmap::new_key_type! {
    pub struct ClientKey;
}

impl Display for ClientKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct WebTransportServerConfig {
    #[derivative(Debug = "ignore")]
    pub native: wtransport::ServerConfig,
    pub version: ProtocolVersion,
    pub lanes: Box<[LaneKind]>,
    pub max_sent_bytes_per_sec: usize,
    pub max_packet_len: usize,
    pub default_packet_cap: usize,
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
    Generic(#[from] BackendError),
}

#[derive(Derivative, thiserror::Error)]
#[derivative(Debug(bound = "packet::SendError<P::S2C>: Debug, packet::RecvError<P::C2S>: Debug"))]
pub enum WebTransportServerError<P>
where
    P: TransportProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryIntoBytes,
{
    #[error("already open")]
    AlreadyOpen,
    #[error("already closed")]
    AlreadyClosed,
    #[error("not connected")]
    NotConnected,
    #[error("backend closed")]
    BackendClosed,

    #[error(transparent)]
    Backend(#[from] ServerBackendError),
    #[error(transparent)]
    Send(#[from] packet::SendError<P::S2C>),
    #[error(transparent)]
    Recv(#[from] packet::RecvError<P::C2S>),
}
