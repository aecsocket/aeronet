mod backend;

use std::{fmt::Debug, future::Future};

use aeronet::{
    error::pretty_error,
    lane::OnLane,
    message::{TryFromBytes, TryIntoBytes},
    protocol::{ProtocolVersion, TransportProtocol},
};
use aeronet_proto::{packet, seq::Seq};
use derivative::Derivative;
use futures::channel::oneshot;
use tracing::debug;
use xwt_core::utils::maybe;

use crate::error::BackendError;

#[cfg(target_family = "wasm")]
type NativeConfig = web_sys::WebTransportOptions;
#[cfg(not(target_family = "wasm"))]
type NativeConfig = wtransport::ClientConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientMessageKey {
    msg_seq: Seq,
}

#[derive(Derivative, thiserror::Error)]
#[derivative(Debug(bound = "packet::SendError<P::C2S>: Debug, packet::RecvError<P::S2C>: Debug"))]
pub enum WebTransportClientError<P>
where
    P: TransportProtocol,
    P::C2S: TryIntoBytes,
    P::S2C: TryFromBytes,
{
    #[error("already disconnected")]
    AlreadyDisconnected,

    #[error(transparent)]
    Backend(#[from] BackendError),
    #[error(transparent)]
    Send(#[from] packet::SendError<P::C2S>),
    #[error(transparent)]
    Recv(#[from] packet::RecvError<P::S2C>),
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct WebTransportClient<P: TransportProtocol> {
    inner: Inner<P>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
enum Inner<P: TransportProtocol> {
    #[derivative(Default)]
    Disconnected,
    Connecting {
        recv_err: oneshot::Receiver<BackendError>,
    },
    Connected {
        recv_err: oneshot::Receiver<BackendError>,
        packets: packet::Packets<P::C2S, P::S2C>,
    },
}

impl<P> WebTransportClient<P>
where
    P: TransportProtocol,
    P::C2S: TryIntoBytes + OnLane,
    P::S2C: TryFromBytes + OnLane,
{
    pub fn disconnected() -> Self {
        Self {
            inner: Inner::Disconnected,
        }
    }

    pub fn disconnect(&mut self) -> Result<(), WebTransportClientError<P>> {
        match self.inner {
            Inner::Disconnected => Err(WebTransportClientError::AlreadyDisconnected),
            _ => {
                self.inner = Inner::Disconnected;
                Ok(())
            }
        }
    }

    pub fn connect_new(
        config: NativeConfig,
        url: impl Into<String>,
    ) -> (Self, impl Future<Output = ()> + maybe::Send) {
        let url = url.into();
        let (send_err, recv_err) = oneshot::channel();
        let (send_connected, recv_connected) = oneshot::channel();
        let backend = async move {
            match backend::open(
                config,
                url,
                ProtocolVersion(0), /* TODO */
                send_connected,
            )
            .await
            {
                Ok(_) => unreachable!(),
                Err(BackendError::FrontendClosed) => {
                    debug!("Connection closed");
                }
                Err(err) => {
                    debug!("Connection closed: {:#}", pretty_error(&err));
                    let _ = send_err.send(err);
                }
            }
        };
        (
            Self {
                inner: Inner::Connecting { recv_err },
            },
            backend,
        )
    }
}
