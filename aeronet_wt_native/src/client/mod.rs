mod backend;
mod config;
mod wrapper;

pub use {config::WebTransportClientConfig, wrapper::*};

use std::{fmt::Debug, future::Future, marker::PhantomData, net::SocketAddr, task::Poll};

use aeronet::{LaneProtocol, OnLane, TransportProtocol, TryAsBytes, TryFromBytes};
use derivative::Derivative;
use futures::channel::oneshot;

use crate::{
    shared::{ConnectionFrontend, Lanes},
    BackendError, ConnectionInfo,
};

type WebTransportError<P> =
    crate::WebTransportError<<P as TransportProtocol>::C2S, <P as TransportProtocol>::S2C>;

type ClientEvent<P> = aeronet::ClientEvent<P, WebTransportError<P>>;

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct ConnectingClient<P> {
    recv_conn: oneshot::Receiver<Result<ConnectedClientInner, BackendError>>,
    _phantom: PhantomData<P>,
}

#[derive(Debug)]
struct ConnectedClientInner {
    conn: ConnectionFrontend,
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
    ) -> (Self, impl Future<Output = ()> + Send) {
        let config = config.into();
        let (send_conn, recv_conn) = oneshot::channel();
        let frontend = Self {
            recv_conn,
            _phantom: PhantomData,
        };
        let backend = backend::connect(config, send_conn);
        (frontend, backend)
    }

    pub fn poll(&mut self) -> Poll<Result<ConnectedClient<P>, WebTransportError<P>>> {
        match self.recv_conn.try_recv() {
            Ok(None) => Poll::Pending,
            Ok(Some(Ok(inner))) => Poll::Ready(Ok(ConnectedClient {
                conn: inner.conn,
                local_addr: inner.local_addr,
                lanes: Lanes::new(),
                _phantom: PhantomData,
            })),
            Ok(Some(Err(err))) => Poll::Ready(Err(err.into())),
            Err(_) => Poll::Ready(Err(WebTransportError::<P>::backend_closed())),
        }
    }
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct ConnectedClient<P> {
    conn: ConnectionFrontend,
    local_addr: SocketAddr,
    lanes: Lanes<P>,
    _phantom: PhantomData<P>,
}

impl<P> ConnectedClient<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
{
    #[must_use]
    pub fn connection_info(&self) -> ConnectionInfo {
        self.conn.info.clone()
    }

    pub fn send(&mut self, msg: impl Into<P::C2S>) -> Result<(), WebTransportError<P>> {
        let msg: P::C2S = msg.into();
        let msg_bytes = msg.try_as_bytes().map_err(WebTransportError::<P>::Encode)?;
        self.lanes
            .send(msg_bytes.as_ref(), msg.lane(), &mut self.conn)
            .map_err(WebTransportError::<P>::Backend)
    }

    pub fn poll(&mut self) -> (Vec<ClientEvent<P>>, Result<(), WebTransportError<P>>) {
        let mut events = Vec::new();
        let result = self._poll(&mut events);
        (events, result)
    }

    fn _poll(&mut self, events: &mut Vec<ClientEvent<P>>) -> Result<(), WebTransportError<P>> {
        self.conn.update();
        self.lanes.update();
        while let Some(msg) = self.lanes.recv(&mut self.conn)? {
            events.push(ClientEvent::Recv { msg });
        }

        self.conn
            .recv_err()
            .map_err(WebTransportError::<P>::Backend)
    }
}
