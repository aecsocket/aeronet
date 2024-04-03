use std::{future::Future, marker::PhantomData};

use aeronet::{
    error::pretty_error,
    lane::{LaneKind, OnLane},
    message::{TryFromBytes, TryIntoBytes},
    protocol::TransportProtocol,
};
use derivative::Derivative;
use futures::channel::{mpsc, oneshot};
use tracing::debug;

use crate::error::BackendError;

use super::{backend, ServerBackendError, WebTransportServerConfig, WebTransportServerError};

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct WebTransportServer<P: TransportProtocol> {
    inner: Inner<P>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
enum Inner<P: TransportProtocol> {
    #[derivative(Default)]
    Closed,
    Opening(Opening),
    Open(Open<P>),
}

#[derive(Debug)]
struct FrontendConfig {
    lanes: Box<[LaneKind]>,
    max_sent_bytes_per_sec: usize,
    max_packet_len: usize,
    default_packet_cap: usize,
}

#[derive(Debug)]
struct Opening {
    config: FrontendConfig,
    recv_err: oneshot::Receiver<ServerBackendError>,
    recv_open: oneshot::Receiver<backend::Open>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
struct Open<P: TransportProtocol> {
    recv_connecting: mpsc::Receiver<backend::Connecting>,
    _phantom: PhantomData<P>,
}

impl<P> WebTransportServer<P>
where
    P: TransportProtocol,
    P::C2S: TryFromBytes + OnLane,
    P::S2C: TryIntoBytes + OnLane,
{
    #[must_use]
    pub fn closed() -> Self {
        Self {
            inner: Inner::Closed,
        }
    }

    pub fn close(&mut self) -> Result<(), WebTransportServerError<P>> {
        if let Inner::Closed = self.inner {
            return Err(WebTransportServerError::AlreadyClosed);
        }

        self.inner = Inner::Closed;
        Ok(())
    }

    pub fn open_new(config: WebTransportServerConfig) -> (Self, impl Future<Output = ()> + Send) {
        let WebTransportServerConfig {
            native: native_config,
            version,
            lanes,
            max_sent_bytes_per_sec,
            max_packet_len,
            default_packet_cap,
        } = config;
        let (send_err, recv_err) = oneshot::channel::<ServerBackendError>();
        let (send_open, recv_open) = oneshot::channel::<backend::Open>();
        let backend = async move {
            let Err(err) = backend::start(native_config, version, send_open).await else {
                unreachable!()
            };
            match err {
                ServerBackendError::Generic(BackendError::FrontendClosed) => {
                    debug!("Connection closed");
                }
                err => {
                    debug!("Connection closed: {:#}", pretty_error(&err));
                    let _ = send_err.send(err);
                }
            }
        };
        (
            Self {
                inner: Inner::Opening(Opening {
                    config: FrontendConfig {
                        lanes,
                        max_sent_bytes_per_sec,
                        max_packet_len,
                        default_packet_cap,
                    },
                    recv_err,
                    recv_open,
                }),
            },
            backend,
        )
    }
}
