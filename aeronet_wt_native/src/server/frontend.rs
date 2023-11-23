use std::future::Future;

use aeronet::{Message, TryFromBytes, TryIntoBytes, OnChannel, ChannelKey};
use tokio::sync::oneshot;
use wtransport::ServerConfig;

use crate::ClientKey;

use super::{OpeningServer, OpenServer, WebTransportError, backend};

pub enum ServerEvent<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    Incoming(ClientKey),
    Accepted(ClientKey),
    Connected(ClientKey),
    Recv(ClientKey, C2S),
    Disconnected(ClientKey, WebTransportError<C2S, S2C, C>),
}

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct WebTransportServer<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    state: Server<C2S, S2C, C>,
}

#[derive(Debug, Default)]
enum Server<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    #[default]
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
    pub fn new_open(config: ServerConfig) -> (Self, impl Future<Output = ()> + Send) {
        let (send_open, recv_open) = oneshot::channel();
        let state = Server::Opening(OpeningServer { recv_open });
        (
            Self { state },
            backend::listen::<C2S, S2C, C>(config, send_open),
        )
    }

    pub fn open(
        &mut self,
        config: ServerConfig,
    ) -> Result<impl Future<Output = ()> + Send, WebTransportError<C2S, S2C, C>> {
        let Server::Closed = self.state else {
            return Err(WebTransportError::BackendOpen);
        };

        let (this, backend) = Self::new_open(config);
        *self = this;
        Ok(backend)
    }
}

pub enum EventIter<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    None,
    One(ServerEvent<C2S, ClientKey, WebTransportError<C2S, S2C, C>>),
    Many(Vec<ServerEvent<C2S, ClientKey, WebTransportError<C2S, S2C, C>>>),
}

impl<C2S, S2C, C> Iterator for EventIter<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    type Item = ServerEvent<C2S, ClientKey, WebTransportError<C2S, S2C, C>>;

    fn next(&mut self) -> Option<Self::Item> {
        match mem::replace(self, Self::None) {
            Self::None => None,
            Self::One(event) => Some(event),
        }
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

    type RecvIter<'a> = EventIter<C2S, S2C, C>;

    fn connection_info(&self, client: Self::Client) -> Option<Self::ConnectionInfo> {
        let Server::Open(server) = &self.state else {
            return None;
        };

        server.clients.get(client).and_then(|client| match client {
            Client::Connected(client) => Some(client.info.clone()),
            _ => None,
        })
    }

    fn send<M: Into<S2C>>(
        &mut self,
        to: Self::Client,
        msg: M,
    ) -> Result<(), WebTransportError<C2S, S2C, C>> {
        match &mut self.state {
            Server::Closed => Err(WebTransportError::BackendClosed),
            Server::Opening(..) => Err(WebTransportError::BackendClosed),
            Server::Open(server) => {
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
        }
    }

    fn recv(&mut self) -> Self::RecvIter<'_> {
        match &mut self.state {
            Server::Closed => EventIter::None,
            Server::Opening(server) => match server.recv_open.try_recv() {
                Ok(Ok(open)) => {
                    self.state = Server::Open(open);
                    EventIter::One(ServerEvent::Opened)
                }
                Ok(Err(err)) => {
                    self.state = Server::Closed;
                    EventIter::One(ServerEvent::Closed {
                        reason: WebTransportError::BackendClosed,
                    })
                }
                Err(oneshot::error::TryRecvError::Empty) => EventIter::None,
                Err(oneshot::error::TryRecvError::Closed) => {
                    self.state = Server::Closed;
                    EventIter::One(ServerEvent::Closed {
                        reason: WebTransportError::BackendClosed,
                    })
                }
            },
            Server::Open(server) => {
                match server.recv_client.try_recv() {
                    Ok(client) => {
                        server.clients.insert(Client::Pending(client));
                    }
                    Err(mpsc::error::TryRecvError::Empty) => {}
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        self.state = Server::Closed;
                        return EventIter::One(ServerEvent::Closed {
                            reason: WebTransportError::BackendClosed,
                        });
                    }
                }

                let mut events = Vec::new();
                for (_, client) in server.clients.iter() {
                    match client {
                        Client::Pending(client) => client.recv_incoming.try_recv(),
                    }
                }
            }
        }
    }

    fn disconnect(&mut self, target: Self::Client) -> Result<(), Self::Error> {
        let Server::Open(server) = &mut self.state else {
            return Err(WebTransportError::BackendClosed);
        };

        match server.clients.remove(target) {
            Some(_) => Ok(()),
            None => Err(WebTransportError::NoClient(target)),
        }
    }
}
