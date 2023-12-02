use std::{future::Future, task::Poll};

use aeronet::{ChannelKey, Message, OnChannel, TransportServer, TryFromBytes, TryIntoBytes};
use tokio::sync::oneshot;
use wtransport::ServerConfig;

use crate::{ClientKey, EndpointInfo, ServerEvent, OpeningServer, OpenServer};

use super::{WebTransportError, Client, backend};

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub enum WebTransportServer<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    Closed,
    Opening(OpeningServer<C2S, S2C, C>),
    Open(OpenServer<C2S, S2C, C>),
}

impl<C2S, S2C, C> WebTransportServer<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    pub fn open(config: ServerConfig) -> (Self, impl Future<Output = ()> + Send) {
        let (send_open, recv_open) = oneshot::channel();
        let frontend = Self::Opening(OpeningServer { recv_open });
        let backend = backend::start(config, send_open);
        (frontend, backend)
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

    type RecvIter<'a> = ServerEventIter<C2S, S2C, C>;

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
            WebTransportServer::Closed => ServerEventIter::None,
            WebTransportServer::Opening(server) => match server.poll() {
                Poll::Pending => ServerEventIter::None,
                Poll::Ready(Ok(open)) => {
                    *self = WebTransportServer::Open(open);
                    ServerEventIter::from(ServerEvent::Opened)
                }
                Poll::Ready(Err(cause)) => {
                    *self = WebTransportServer::Closed;
                    ServerEventIter::from(ServerEvent::Closed { cause })
                }
            }
            WebTransportServer::Open(server) => match server.recv() {
                Ok(events) => ServerEventIter::from(events),
                Err(cause) => {
                    *self = WebTransportServer::Closed;
                    ServerEventIter::from(ServerEvent::Closed { cause })
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

pub enum ServerEventIter<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    None,
    One(std::iter::Once<ServerEvent<C2S, S2C, C>>),
    Some(std::vec::IntoIter<ServerEvent<C2S, S2C, C>>),
}

impl<C2S, S2C, C> From<ServerEvent<C2S, S2C, C>> for ServerEventIter<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    fn from(value: ServerEvent<C2S, S2C, C>) -> Self {
        Self::One(std::iter::once(value))
    }
}

impl<C2S, S2C, C> From<std::vec::IntoIter<ServerEvent<C2S, S2C, C>>> for ServerEventIter<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    fn from(value: std::vec::IntoIter<ServerEvent<C2S, S2C, C>>) -> Self {
        Self::Some(value)
    }
}

impl<C2S, S2C, C> Iterator for ServerEventIter<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    type Item = ServerEvent<C2S, S2C, C>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::None => None,
            Self::One(iter) => iter.next(),
            Self::Some(iter) => iter.next(),
        }
    }
}
