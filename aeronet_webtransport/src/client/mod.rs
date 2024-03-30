mod backend;
mod wrapper;

use aeronet_protocol::seq::Seq;
pub use wrapper::*;

use std::{future::Future, marker::PhantomData, task::Poll};

use aeronet::{
    lane::{LaneConfig, OnLane},
    message::{TryFromBytes, TryIntoBytes},
    protocol::{ProtocolVersion, TransportProtocol},
};
use derivative::Derivative;
use futures::channel::oneshot;
use xwt_core::utils::maybe;

use crate::{shared::ConnectionFrontend, BackendError, ConnectionInfo};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientMessageKey {
    msg_seq: Seq,
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
    P::C2S: TryIntoBytes + TryFromBytes + OnLane,
    P::S2C: TryIntoBytes + TryFromBytes + OnLane,
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
pub struct ConnectedClient<P: TransportProtocol> {
    conn: ConnectionFrontend<P::C2S, P::S2C>,
    #[cfg(not(target_family = "wasm"))]
    local_addr: std::net::SocketAddr,
    #[derivative(Debug = "ignore")]
    _phantom: PhantomData<P>,
}

impl<P> ConnectedClient<P>
where
    P: TransportProtocol,
    P::C2S: TryIntoBytes + TryFromBytes + OnLane,
    P::S2C: TryIntoBytes + TryFromBytes + OnLane,
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

    pub fn send(
        &mut self,
        msg: impl Into<P::C2S>,
    ) -> Result<ClientMessageKey, WebTransportError<P>> {
        self.conn
            .buffer_send(msg.into())
            .map(|msg_seq| ClientMessageKey { msg_seq })
    }

    pub fn poll(&mut self) -> (Vec<ClientEvent<P>>, Result<(), WebTransportError<P>>) {
        let mut events = Vec::new();
        let result = self.do_poll(&mut events);
        (events, result)
    }

    fn do_poll(&mut self, events: &mut Vec<ClientEvent<P>>) -> Result<(), WebTransportError<P>> {
        self.conn.poll()?;
        while let Some(mut packet) = self.conn.recv() {
            for msg_seq in self.conn.read_acks(&mut packet)? {
                events.push(ClientEvent::Ack {
                    msg_key: ClientMessageKey { msg_seq },
                });
            }
            for result in self.conn.read_frags(packet) {
                let msg = result?;
                events.push(ClientEvent::Recv { msg });
            }
        }
        self.conn.recv_err()?;
        Ok(())
    }
}
