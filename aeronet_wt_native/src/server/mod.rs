mod backend;

use std::{future::Future, io, net::SocketAddr, mem};

use aeronet::{
    ChannelKey, Message, OnChannel, ServerEvent, TransportServer, TryFromBytes, TryIntoBytes,
};
use derivative::Derivative;
use slotmap::SlotMap;
use tokio::sync::{mpsc, oneshot};
use wtransport::ServerConfig;

use crate::{ClientKey, EndpointInfo};

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
            backend::listen::<C2S, S2C, C>(config, send_open)
        )
    }

    pub fn open(&mut self, config: ServerConfig) -> Result<impl Future<Output = ()> + Send, WebTransportError<C2S, S2C, C>> {
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
    Many(Vec<ServerEvent<C2S, ClientKey, WebTransportError<C2S, S2C, C>>>)
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
            Self::Many()
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
                    EventIter::One(ServerEvent::Closed { reason: WebTransportError::BackendClosed })
                }
                Err(oneshot::error::TryRecvError::Empty) => EventIter::None,
                Err(oneshot::error::TryRecvError::Closed) => {
                    self.state = Server::Closed;
                    EventIter::One(ServerEvent::Closed { reason: WebTransportError::BackendClosed })
                }
            },
            Server::Open(server) => {
                match server.recv_client.try_recv() {
                    Ok(client) => {
                        server.clients.insert(Client::Pending(client));
                    }
                    Err(mpsc::error::TryRecvError::Empty) => {},
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        self.state = Server::Closed;
                        return EventIter::One(ServerEvent::Closed { reason: WebTransportError::BackendClosed });
                    }
                }

                let mut events = Vec::new();
                for (_, client) in server.clients.iter() {
                    match client {
                        Client::Pending(client) => {
                            client.recv_incoming.try_recv()
                        }
                    }
                }

            },
        }
    }

    fn disconnect(&mut self, target: Self::Client) -> Result<(), Self::Error> {
        let Server::Open(server) = &mut self.state else {
            return Err(WebTransportError::BackendClosed);
        };

        match server.clients.remove(target) {
            Some(_) => Ok(()),
            None => Err(WebTransportError::NoClient(target))
        }
    }
}

// state machine

type WebTransportError<C2S, S2C, C> = crate::WebTransportError<S2C, C2S, C>;

#[derive(Derivative)]
#[derivative(Debug)]
struct OpeningServer<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    #[derivative(Debug = "ignore")]
    recv_open: oneshot::Receiver<OpenResult<C2S, S2C, C>>,
}

type OpenResult<C2S, S2C, C> = Result<OpenServer<C2S, S2C, C>, WebTransportError<C2S, S2C, C>>;

#[derive(Derivative)]
#[derivative(Debug)]
struct OpenServer<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    local_addr: Result<SocketAddr, io::Error>,
    clients: SlotMap<ClientKey, Client<C2S, S2C, C>>,
    #[derivative(Debug = "ignore")]
    recv_client: mpsc::UnboundedReceiver<PendingClient<C2S, S2C, C>>,
    // TODO this needs to be improved
    #[derivative(Debug = "ignore")]
    send_closed: mpsc::Sender<()>,
}

#[derive(Derivative)]
#[derivative(Debug)]
struct PendingClient<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    #[derivative(Debug = "ignore")]
    recv_incoming: oneshot::Receiver<IncomingClientResult<C2S, S2C, C>>,
}

type IncomingClientResult<C2S, S2C, C> =
    Result<IncomingClient<C2S, S2C, C>, WebTransportError<C2S, S2C, C>>;

#[derive(Derivative)]
#[derivative(Debug)]
struct IncomingClient<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    authority: String,
    path: String,
    origin: Option<String>,
    user_agent: Option<String>,
    #[derivative(Debug = "ignore")]
    recv_connected: oneshot::Receiver<ConnectedClientResult<C2S, S2C, C>>,
}

type ConnectedClientResult<C2S, S2C, C> =
    Result<ConnectedClient<C2S, S2C, C>, WebTransportError<C2S, S2C, C>>;

#[derive(Derivative)]
#[derivative(Debug)]
struct ConnectedClient<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    info: EndpointInfo,
    #[derivative(Debug = "ignore")]
    recv_info: mpsc::UnboundedReceiver<EndpointInfo>,
    #[derivative(Debug = "ignore")]
    recv_c2s: mpsc::UnboundedReceiver<C2S>,
    #[derivative(Debug = "ignore")]
    send_s2c: mpsc::UnboundedSender<S2C>,
    #[derivative(Debug = "ignore")]
    recv_err: oneshot::Receiver<WebTransportError<C2S, S2C, C>>,
}

#[derive(Debug)]
enum Client<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    Pending(PendingClient<C2S, S2C, C>),
    Incoming(IncomingClient<C2S, S2C, C>),
    Connected(ConnectedClient<C2S, S2C, C>),
}
