mod backend;
mod frontend;

pub use frontend::*;

use std::fmt::Debug;

use aeronet::{
    lane::LaneKind,
    message::{TryFromBytes, TryIntoBytes},
    protocol::{ProtocolVersion, TransportProtocol},
};
use aeronet_proto::packet;
use derivative::Derivative;

use crate::shared;

#[cfg(target_family = "wasm")]
pub type NativeConfig = web_sys::WebTransportOptions;
#[cfg(not(target_family = "wasm"))]
pub type NativeConfig = wtransport::ClientConfig;

#[derive(Derivative)]
#[derivative(Debug)]
pub struct ClientConfig {
    #[derivative(Debug = "ignore")]
    pub native: NativeConfig,
    pub version: ProtocolVersion,
    pub lanes: Box<[LaneKind]>,
    pub bandwidth: usize,
    pub max_packet_len: usize,
    pub default_packet_cap: usize,
}

impl ClientConfig {
    pub fn new(native: impl Into<NativeConfig>) -> Self {
        Self {
            native: native.into(),
            version: ProtocolVersion::default(),
            lanes: Box::default(),
            bandwidth: shared::DEFAULT_BANDWIDTH,
            max_packet_len: shared::DEFAULT_MTU,
            default_packet_cap: shared::DEFAULT_MTU,
        }
    }
}

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        use crate::error::JsError;

        type EndpointConnectError = JsError;
        type ConnectingError = JsError;
    } else {
        use crate::ty;

        type EndpointConnectError = <ty::Endpoint as xwt_core::EndpointConnect>::Error;
        type ConnectingError = <ty::Connecting as xwt_core::Connecting>::Error;
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("failed to start connecting")]
    StartConnecting(#[source] EndpointConnectError),
    #[error("failed to await connection")]
    AwaitConnection(#[source] ConnectingError),
    #[error(transparent)]
    Generic(#[from] shared::BackendError),
}

#[derive(Derivative, thiserror::Error)]
#[derivative(Debug(bound = "packet::SendError<P::C2S>: Debug, packet::RecvError<P::S2C>: Debug"))]
pub enum ClientError<P>
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

    #[error(transparent)]
    Backend(#[from] BackendError),
    #[error(transparent)]
    Send(#[from] packet::SendError<P::C2S>),
    #[error(transparent)]
    Recv(#[from] packet::RecvError<P::S2C>),
}
