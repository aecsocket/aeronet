mod backend;
mod wrapper;

pub use wrapper::*;

use std::{future::Future, marker::PhantomData, task::Poll};

use aeronet::{
    LaneConfig, MessageState, OnLane, ProtocolVersion, TransportProtocol, TryAsBytes, TryFromBytes,
};
use derivative::Derivative;
use futures::channel::oneshot;
use xwt_core::utils::maybe;

use crate::{shared::ConnectionFrontend, BackendError, ConnectionInfo, MessageKey};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientMessageKey {
    key: MessageKey,
}

type WebTransportError<P> =
    crate::WebTransportError<<P as TransportProtocol>::C2S, <P as TransportProtocol>::S2C>;

type ClientEvent<P> = aeronet::client::ClientEvent<P, WebTransportError<P>, ClientMessageKey>;

pub struct WebTransportClientConfig {
    #[cfg(target_family = "wasm")]
    pub native: web_sys::WebTransportOptions,
    #[cfg(not(target_family = "wasm"))]
    pub native: wtransport::ClientConfig,
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
    local_addr: std::net::SocketAddr,
}

impl<P> ConnectingClient<P>
where
    P: TransportProtocol,
    P::C2S: TryAsBytes + TryFromBytes + OnLane,
    P::S2C: TryAsBytes + TryFromBytes + OnLane,
{
    pub fn connect(
        config: WebTransportClientConfig,
    ) -> (Self, impl Future<Output = ()> + maybe::Send) {
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

#[cfg(target_family = "wasm")]
#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct ConnectedClient<P> {
    conn: ConnectionFrontend,
    #[derivative(Debug = "ignore")]
    _phantom: PhantomData<P>,
}

#[cfg(not(target_family = "wasm"))]
#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct ConnectedClient<P> {
    conn: ConnectionFrontend,
    #[cfg(not(target_family = "wasm"))]
    local_addr: std::net::SocketAddr,
    #[derivative(Debug = "ignore")]
    _phantom: PhantomData<P>,
}

impl<P> ConnectedClient<P>
where
    P: TransportProtocol,
    P::C2S: TryAsBytes + TryFromBytes + OnLane,
    P::S2C: TryAsBytes + TryFromBytes + OnLane,
{
    #[cfg(not(target_family = "wasm"))]
    #[must_use]
    pub fn local_addr(&self) -> std::net::SocketAddr {
        self.local_addr
    }

    #[must_use]
    pub fn connection_info(&self) -> ConnectionInfo {
        self.conn.info.clone()
    }

    #[must_use]
    pub fn message_state(&self, msg_key: ClientMessageKey) -> Option<MessageState> {
        None
    }

    pub fn send(
        &mut self,
        msg: impl Into<P::C2S>,
    ) -> Result<ClientMessageKey, WebTransportError<P>> {
        self.conn
            .buffer_send(&msg.into())
            .map(|key| ClientMessageKey { key })
    }

    pub fn poll(&mut self) -> (Vec<ClientEvent<P>>, Result<(), WebTransportError<P>>) {
        let mut events = Vec::new();
        let result = self._poll(&mut events);
        (events, result)
    }

    fn _poll(&mut self, events: &mut Vec<ClientEvent<P>>) -> Result<(), WebTransportError<P>> {
        self.conn.poll();
        while let Some(msg) = self.conn.recv()? {
            events.push(ClientEvent::Recv { msg });
        }
        self.conn
            .recv_err()
            .map_err(WebTransportError::<P>::Backend)
    }
}
