mod backend;
mod wrapper;

pub use wrapper::*;

use std::{future::Future, marker::PhantomData, net::SocketAddr, task::Poll};

use aeronet::{
    LaneConfig, LaneProtocol, OnLane, ProtocolVersion, TransportProtocol, TryAsBytes, TryFromBytes,
};
use derivative::Derivative;
use futures::channel::oneshot;
use xwt_core::utils::maybe;

use crate::{shared::ConnectionFrontend, BackendError, ConnectionInfo};

type WebTransportError<P> =
    crate::WebTransportError<<P as TransportProtocol>::C2S, <P as TransportProtocol>::S2C>;

type ClientEvent<P> = aeronet::client::ClientEvent<P, WebTransportError<P>>;

#[cfg(target_family = "wasm")]
type NativeClientConfig = web_sys::WebTransportOptions;

#[cfg(not(target_family = "wasm"))]
type NativeClientConfig = wtransport::ClientConfig;

pub struct WebTransportClientConfig {
    pub native: NativeClientConfig,
    pub version: ProtocolVersion,
    pub max_packet_len: usize,
    pub lanes: Vec<LaneConfig>,
    pub url: String,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct ConnectingClient<P> {
    recv_conn: oneshot::Receiver<Result<ConnectedInner, BackendError>>,
    #[derivative(Debug = "ignore")]
    _phantom: PhantomData<P>,
}

#[derive(Debug)]
struct ConnectedInner {
    conn: ConnectionFrontend,
    #[cfg(not(target_family = "wasm"))]
    local_addr: SocketAddr,
}

impl<P> ConnectingClient<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
{
    pub fn connect(
        config: impl Into<WebTransportClientConfig>,
    ) -> (Self, impl Future<Output = ()> + maybe::Send) {
        let config = config.into();
        let (send_conn, recv_conn) = oneshot::channel();
        (
            Self {
                recv_conn,
                _phantom: PhantomData,
            },
            backend::connect(config, send_conn),
        )
    }

    pub fn poll(&mut self) -> Poll<Result<ConnectedClient<P>, WebTransportError<P>>> {
        match self.recv_conn.try_recv() {
            Ok(None) => Poll::Pending,
            Ok(Some(Ok(inner))) => Poll::Ready(Ok(ConnectedClient {
                conn: inner.conn,
                #[cfg(not(target_family = "wasm"))]
                local_addr: inner.local_addr,
                _phantom: PhantomData,
            })),
            Ok(Some(Err(err))) => Poll::Ready(Err(err.into())),
            Err(_) => Poll::Ready(Err(WebTransportError::<P>::Backend(BackendError::Closed))),
        }
    }
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct ConnectedClient<P> {
    conn: ConnectionFrontend,
    #[cfg(not(target_family = "wasm"))]
    local_addr: SocketAddr,
    _phantom: PhantomData<P>,
}

impl<P> ConnectedClient<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
{
    #[cfg(not(target_family = "wasm"))]
    #[must_use]
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    #[must_use]
    pub fn connection_info(&self) -> ConnectionInfo {
        self.conn.info.clone()
    }

    pub fn send(&mut self, msg: impl Into<P::C2S>) -> Result<(), WebTransportError<P>> {
        self.conn.send(msg.into())
    }

    pub fn poll(&mut self) -> (Vec<ClientEvent<P>>, Result<(), WebTransportError<P>>) {
        let mut events = Vec::new();
        let result = self._poll(&mut events);
        (events, result)
    }

    fn _poll(&mut self, events: &mut Vec<ClientEvent<P>>) -> Result<(), WebTransportError<P>> {
        self.conn.update();
        while let Some(msg) = self.conn.recv()? {
            events.push(ClientEvent::Recv { msg });
        }
        self.conn
            .recv_err()
            .map_err(WebTransportError::<P>::Backend)
    }
}