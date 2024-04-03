mod backend;
mod frontend;

pub use frontend::*;

use std::{borrow::Borrow, fmt::Debug};

use aeronet::{
    lane::LaneKind,
    message::{TryFromBytes, TryIntoBytes},
    protocol::{ProtocolVersion, TransportProtocol},
};
use aeronet_proto::{packet, seq::Seq};
use derivative::Derivative;

use crate::{error::BackendError, transport::MTU};

#[cfg(target_family = "wasm")]
type NativeConfig = web_sys::WebTransportOptions;
#[cfg(not(target_family = "wasm"))]
type NativeConfig = wtransport::ClientConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientMessageKey {
    msg_seq: Seq,
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct WebTransportClientConfig {
    #[derivative(Debug = "ignore")]
    pub native: NativeConfig,
    pub version: ProtocolVersion,
    pub lanes: Box<[LaneKind]>,
    pub max_sent_bytes_per_sec: usize,
    pub max_packet_len: usize,
    pub default_packet_cap: usize,
}

impl Default for WebTransportClientConfig {
    fn default() -> Self {
        Self {
            native: NativeConfig::default(),
            version: ProtocolVersion::default(),
            lanes: Box::new([]),
            max_sent_bytes_per_sec: 28_800_000, // TODO document
            max_packet_len: MTU,
            default_packet_cap: MTU,
        }
    }
}

impl WebTransportClientConfig {
    pub fn new(
        native: impl Into<NativeConfig>,
        version: ProtocolVersion,
        lanes: impl IntoIterator<Item = impl Borrow<LaneKind>>,
    ) -> Self {
        Self {
            native: native.into(),
            version,
            lanes: lanes.into_iter().map(|kind| *kind.borrow()).collect(),
            ..Default::default()
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
pub enum ClientBackendError {
    #[error("failed to start connecting")]
    StartConnecting(#[source] EndpointConnectError),
    #[error("failed to await connection")]
    AwaitConnection(#[source] ConnectingError),
    #[error(transparent)]
    Generic(#[from] BackendError),
}

#[derive(Derivative, thiserror::Error)]
#[derivative(Debug(bound = "packet::SendError<P::C2S>: Debug, packet::RecvError<P::S2C>: Debug"))]
pub enum WebTransportClientError<P>
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
    Backend(#[from] ClientBackendError),
    #[error(transparent)]
    Send(#[from] packet::SendError<P::C2S>),
    #[error(transparent)]
    Recv(#[from] packet::RecvError<P::S2C>),
}
