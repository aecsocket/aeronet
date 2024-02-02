use std::{fmt::Debug, net::SocketAddr, task::Poll};

use aeronet::{
    ClientKey, ClientState, LaneProtocol, LocalAddr, OnLane, ServerEvent, ServerState,
    ServerTransport, TransportProtocol, TryAsBytes, TryFromBytes,
};
use derivative::Derivative;
use either::Either;

use crate::{ConnectionInfo, OpenServer, OpeningServer, RemoteConnectingClientInfo};

use super::WebTransportError;

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
pub enum WebTransportServer<P: TransportProtocol> {
    #[derivative(Default)]
    Closed,
    Opening(OpeningServer<P>),
    Open(OpenServer<P>),
}

trait FooProtocol
where
    Self: LaneProtocol,
    Self::C2S: TryAsBytes + TryFromBytes + OnLane<Lane = Self::Lane>,
    Self::S2C: TryAsBytes + TryFromBytes + OnLane<Lane = Self::Lane>,
{
}

impl<P: FooProtocol> WebTransportServer<P> {}

impl<P> ServerTransport<P> for WebTransportServer<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
{
    type Error = WebTransportError<P>;

    type OpeningInfo = ();

    type OpenInfo = OpenServerInfo;

    type ConnectingInfo = RemoteConnectingClientInfo;

    type ConnectedInfo = ConnectionInfo;

    fn state(&self) -> ServerState<Self::OpeningInfo, Self::OpenInfo> {
        match self {
            Self::Closed => ServerState::Closed,
            Self::Opening(server) => ServerState::Opening(()),
            Self::Open(server) => ServerState::Open(OpenServerInfo {
                local_addr: server.local_addr(),
            }),
        }
    }

    fn client_state(
        &self,
        client_key: ClientKey,
    ) -> ClientState<Self::ConnectingInfo, Self::ConnectedInfo> {
        match self {
            Self::Closed | Self::Opening(_) => ClientState::Disconnected,
            Self::Open(server) => server.client_state(client_key),
        }
    }

    fn client_keys(&self) -> impl Iterator<Item = ClientKey> + '_ {
        match self {
            Self::Closed | Self::Opening(_) => Either::Left(std::iter::empty()),
            Self::Open(server) => Either::Right(server.client_keys()),
        }
    }

    fn send(&mut self, client_key: ClientKey, msg: impl Into<P::S2C>) -> Result<(), Self::Error> {
        match self {
            Self::Closed | Self::Opening(_) => Err(WebTransportError::<P>::NotOpen),
            Self::Open(server) => server.send(client_key, msg),
        }
    }

    fn disconnect(&mut self, client_key: ClientKey) -> Result<(), Self::Error> {
        match self {
            Self::Closed | Self::Opening(_) => Err(WebTransportError::<P>::NotOpen),
            Self::Open(server) => server.disconnect(client_key),
        }
    }

    fn update(&mut self) -> impl Iterator<Item = ServerEvent<P, Self::Error>> {
        match self {
            Self::Closed => vec![],
            Self::Opening(server) => match server.poll() {
                Poll::Pending => vec![],
                Poll::Ready(Ok(server)) => {
                    *self = Self::Open(server);
                    vec![ServerEvent::Opened]
                }
                Poll::Ready(Err(reason)) => {
                    *self = Self::Closed;
                    vec![ServerEvent::Closed]
                }
            },
            Self::Open(server) => match server.update() {
                (events, Ok(())) => events,
                (mut events, Err(reason)) => {
                    *self = Self::Closed;
                    events.push(ServerEvent::Closed { reason });
                    events
                }
            },
        }
        .into_iter()
    }
}

#[derive(Debug, Clone)]
pub struct OpenServerInfo {
    pub local_addr: SocketAddr,
}

impl LocalAddr for OpenServerInfo {
    fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }
}
