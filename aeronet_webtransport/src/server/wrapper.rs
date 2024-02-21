use std::{fmt::Debug, future::Future, net::SocketAddr, task::Poll};

use aeronet::{
    client::{ClientKey, ClientState},
    server::{ServerState, ServerTransport},
    LocalAddr, OnLane, TransportProtocol, TryAsBytes, TryFromBytes,
};
use derivative::Derivative;
use either::Either;

use crate::{
    ConnectionInfo, OpenServer, OpeningServer, RemoteRequestingInfo, WebTransportServerConfig,
};

use super::{ServerEvent, WebTransportError};

/// [`ServerTransport`] implementation using the WebTransport protocol.
///
/// See the [crate-level docs](crate).
#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub enum WebTransportServer<P> {
    /// See [`ServerState::Closed`].
    #[derivative(Default)]
    Closed,
    /// See [`ServerState::Opening`].
    Opening(OpeningServer<P>),
    /// See [`ServerState::Open`].
    Open(OpenServer<P>),
}

impl<P> WebTransportServer<P>
where
    P: TransportProtocol,
    P::C2S: TryAsBytes + TryFromBytes + OnLane,
    P::S2C: TryAsBytes + TryFromBytes + OnLane,
{
    /// See [`OpeningServer::open`].
    pub fn open_new(
        config: impl Into<WebTransportServerConfig>,
    ) -> (Self, impl Future<Output = ()> + Send) {
        let config = config.into();
        let (server, backend) = OpeningServer::open(config);
        (Self::Opening(server), backend)
    }

    /// See [`OpeningServer::open`].
    ///
    /// # Errors
    ///
    /// Errors if `self` is not [`WebTransportServer::Closed`].
    pub fn open(
        &mut self,
        config: impl Into<WebTransportServerConfig>,
    ) -> Result<impl Future<Output = ()> + Send, WebTransportError<P>> {
        match self {
            Self::Closed => {
                let config = config.into();
                let (this, backend) = Self::open_new(config);
                *self = this;
                Ok(backend)
            }
            Self::Opening(_) | Self::Open(_) => Err(WebTransportError::<P>::AlreadyOpen),
        }
    }

    /// Closes the server, immediately and disgracefully shutting down all
    /// client connections.
    ///
    /// # Errors
    ///
    /// Errors if `self` is [`WebTransportServer::Closed`].
    pub fn close(&mut self) -> Result<(), WebTransportError<P>> {
        match self {
            Self::Closed => Err(WebTransportError::<P>::AlreadyClosed),
            Self::Opening(_) | Self::Open(_) => {
                *self = Self::Closed;
                Ok(())
            }
        }
    }

    /// See [`OpenServer::accept_request`].
    pub fn accept_request(&mut self, client_key: ClientKey) -> Result<(), WebTransportError<P>> {
        match self {
            Self::Closed | Self::Opening(_) => Err(WebTransportError::<P>::NotOpen),
            Self::Open(server) => server.accept_request(client_key),
        }
    }

    /// See [`OpenServer::reject_request`].
    pub fn reject_request(&mut self, client_key: ClientKey) -> Result<(), WebTransportError<P>> {
        match self {
            Self::Closed | Self::Opening(_) => Err(WebTransportError::<P>::NotOpen),
            Self::Open(server) => server.reject_request(client_key),
        }
    }
}

impl<P> ServerTransport<P> for WebTransportServer<P>
where
    P: TransportProtocol,
    P::C2S: TryAsBytes + TryFromBytes + OnLane,
    P::S2C: TryAsBytes + TryFromBytes + OnLane,
{
    type Error = WebTransportError<P>;

    type OpeningInfo = ();

    type OpenInfo = ServerOpenInfo;

    type ConnectingInfo = RemoteRequestingInfo;

    type ConnectedInfo = ConnectionInfo;

    fn state(&self) -> ServerState<Self::OpeningInfo, Self::OpenInfo> {
        match self {
            Self::Closed => ServerState::Closed,
            Self::Opening(_) => ServerState::Opening(()),
            Self::Open(server) => ServerState::Open(ServerOpenInfo {
                local_addr: server.local_addr(),
            }),
        }
    }

    fn client_state(
        &self,
        client: ClientKey,
    ) -> ClientState<Self::ConnectingInfo, Self::ConnectedInfo> {
        match self {
            Self::Closed | Self::Opening(_) => ClientState::Disconnected,
            Self::Open(server) => server.client_state(client),
        }
    }

    fn client_keys(&self) -> impl Iterator<Item = ClientKey> + '_ {
        match self {
            Self::Closed | Self::Opening(_) => Either::Left(std::iter::empty()),
            Self::Open(server) => Either::Right(server.client_keys()),
        }
    }

    fn send(&mut self, client: ClientKey, msg: impl Into<P::S2C>) -> Result<(), Self::Error> {
        match self {
            Self::Closed | Self::Opening(_) => Err(WebTransportError::<P>::NotOpen),
            Self::Open(server) => server.send(client, msg),
        }
    }

    fn disconnect(&mut self, client: ClientKey) -> Result<(), Self::Error> {
        match self {
            Self::Closed | Self::Opening(_) => Err(WebTransportError::<P>::NotOpen),
            Self::Open(server) => server.disconnect(client),
        }
    }

    fn poll(&mut self) -> impl Iterator<Item = ServerEvent<P>> {
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
                    vec![ServerEvent::Closed { reason }]
                }
            },
            Self::Open(server) => server.poll(),
        }
        .into_iter()
    }
}

/// Info on a [`WebTransportServer`] in the [`ServerState::Open`] state.
#[derive(Debug, Clone)]
pub struct ServerOpenInfo {
    /// See [`LocalAddr`].
    pub local_addr: SocketAddr,
}

impl LocalAddr for ServerOpenInfo {
    fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }
}
