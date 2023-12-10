use std::{future::Future, task::Poll};

use aeronet::{ChannelKey, Message, OnChannel, TransportServer, TryFromBytes, TryIntoBytes};
use wtransport::ServerConfig;

use crate::{ClientKey, EndpointInfo, ServerEvent, OpeningServer, OpenServer};

use super::{WebTransportError, Client};

/// An implementation of [`TransportServer`] using the WebTransport protocol.
/// 
/// See the [crate-level documentation](crate) for implementation details.
#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub enum WebTransportServer<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    /// The server is not attempting to listen to connections.
    Closed,
    /// The server is starting its endpoint, but is not ready to receive
    /// connections yet.
    Opening(OpeningServer<C2S, S2C, C>),
    /// The server is ready to communicate with clients.
    Open(OpenServer<C2S, S2C, C>),
}

impl<C2S, S2C, C> WebTransportServer<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    /// Starts opening a server and wraps it in a [`WebTransportServer`].
    /// 
    /// See [`OpeningServer::open`].
    pub fn open(config: ServerConfig) -> (Self, impl Future<Output = ()> + Send) {
        let (frontend, backend) = OpeningServer::open(config);
        (Self::Opening(frontend), backend)
    }
}

impl<C2S, S2C, C> TransportServer<C2S, S2C> for WebTransportServer<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    type Client = ClientKey;

    type Error = WebTransportError<C2S, S2C, C>;

    type ConnectionInfo = EndpointInfo;

    type Event = ServerEvent<C2S, S2C, C>;

    type RecvIter<'a> = std::vec::IntoIter<Self::Event>;

    fn connection_info(&self, client: Self::Client) -> Option<Self::ConnectionInfo> {
        let WebTransportServer::Open(server) = self else {
            return None;
        };
        let Some(Client::Connected(client)) = server.clients.get(client) else {
            return None;
        };

        Some(client.info.clone())
    }

    fn send<M: Into<S2C>>(
        &mut self,
        to: Self::Client,
        msg: M,
    ) -> Result<(), WebTransportError<C2S, S2C, C>> {
        let WebTransportServer::Open(server) = self else {
            return Err(WebTransportError::BackendClosed);
        };
        let Some(client) = server.clients.get(to) else {
            return Err(WebTransportError::NoClient(to));
        };
        let Client::Connected(client) = client else {
            return Err(WebTransportError::NotConnected(to));
        };

        let msg = msg.into();
        client
            .send_s2c
            .send(msg)
            .map_err(|_| WebTransportError::NotConnected(to))
    }

    fn recv(&mut self) -> Self::RecvIter<'_> {
        match self {
            WebTransportServer::Closed => vec![].into_iter(),
            WebTransportServer::Opening(server) => match server.poll() {
                Poll::Pending => vec![].into_iter(),
                Poll::Ready(Ok(open)) => {
                    *self = WebTransportServer::Open(open);
                    vec![ServerEvent::Opened].into_iter()
                }
                Poll::Ready(Err(cause)) => {
                    *self = WebTransportServer::Closed;
                    vec![ServerEvent::Closed { cause }].into_iter()
                }
            }
            WebTransportServer::Open(server) => match server.recv() {
                Ok(events) => events,
                Err(cause) => {
                    *self = WebTransportServer::Closed;
                    vec![ServerEvent::Closed { cause }].into_iter()
                }
            }
        }
    }

    fn disconnect(&mut self, target: Self::Client) -> Result<(), Self::Error> {
        let WebTransportServer::Open(server) = self else {
            return Err(WebTransportError::BackendClosed);
        };

        match server.clients.remove(target) {
            Some(_) => Ok(()),
            None => Err(WebTransportError::NoClient(target)),
        }
    }
}
