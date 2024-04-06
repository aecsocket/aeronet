mod backend;
mod frontend;

pub use frontend::*;

use std::fmt::Debug;

use aeronet::{lane::LaneKind, message::BytesMapper, protocol::ProtocolVersion};
use aeronet_proto::packet;
use derivative::Derivative;

use crate::shared::{self, WebTransportProtocol};

#[cfg(target_family = "wasm")]
pub type NativeConfig = web_sys::WebTransportOptions;
#[cfg(not(target_family = "wasm"))]
pub type NativeConfig = wtransport::ClientConfig;

#[derive(Derivative)]
#[derivative(Debug(bound = "P::Mapper: Debug"))]
pub struct ClientConfig<P: WebTransportProtocol> {
    #[derivative(Debug = "ignore")]
    pub native: NativeConfig,
    pub version: ProtocolVersion,
    pub lanes_in: Box<[LaneKind]>,
    pub lanes_out: Box<[LaneKind]>,
    pub mapper: P::Mapper,
    pub bandwidth: usize,
    pub max_packet_len: usize,
    pub default_packet_cap: usize,
}

impl<P: WebTransportProtocol> ClientConfig<P> {
    pub fn new(native: impl Into<NativeConfig>, mapper: P::Mapper) -> Self {
        Self {
            native: native.into(),
            version: ProtocolVersion::default(),
            lanes_in: Box::default(),
            lanes_out: Box::default(),
            mapper,
            bandwidth: shared::DEFAULT_BANDWIDTH,
            max_packet_len: shared::DEFAULT_MTU,
            default_packet_cap: shared::DEFAULT_MTU,
        }
    }
}

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        use crate::shared::JsError;

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
    #[error("failed to connect")]
    Connect(#[source] EndpointConnectError),
    #[error("failed to await connection")]
    AwaitConnection(#[source] ConnectingError),
    #[error(transparent)]
    Generic(#[from] shared::BackendError),
}

#[derive(Derivative, thiserror::Error)]
#[derivative(Debug(
    bound = "packet::SendError<<P::Mapper as BytesMapper<P::C2S>>::IntoError>: Debug, packet::RecvError<<P::Mapper as BytesMapper<P::S2C>>::FromError>: Debug"
))]
pub enum ClientError<P: WebTransportProtocol> {
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
    Send(#[from] packet::SendError<<P::Mapper as BytesMapper<P::C2S>>::IntoError>),
    #[error(transparent)]
    Recv(#[from] packet::RecvError<<P::Mapper as BytesMapper<P::S2C>>::FromError>),
}
