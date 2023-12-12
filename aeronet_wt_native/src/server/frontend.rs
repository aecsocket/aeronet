use std::{future::Future, io, net::SocketAddr, task::Poll};

use aeronet::{ChannelKey, Message, OnChannel, TransportServer, TryFromBytes, TryIntoBytes};
use tokio::sync::{mpsc, oneshot};
use wtransport::ServerConfig;

use crate::{ClientKey, EndpointInfo, ServerEvent};

use super::{backend, ClientState, OpenServer, OpenServerResult, OpeningServer, WebTransportError};

/// Implementation of [`TransportServer`] using the WebTransport protocol.
///
/// See the [crate-level docs](crate).
#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct WebTransportServer<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    state: State<C2S, S2C, C>,
}

#[derive(Debug)]
enum State<C2S, S2C, C>
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
    /// Creates a new server which is not open for connections, and is not
    /// starting to open.
    /// 
    /// This is useful if you want to prepare a server, but do not want to open
    /// it up to accepting connections yet.
    /// 
    /// If you want to create a server and start listening for connections
    /// immediately after creation, use [`WebTransportServer::opening`] instead.
    pub fn closed() -> Self {
        Self {
            state: State::Closed,
        }
    }

    /// Creates and starts opening a server.
    ///
    /// This returns:
    /// * the server frontend
    ///   * use this throughout your app to interface with the server
    /// * a [`Future`] for the server's backend task
    ///   * run this on an async runtime as soon as possible
    pub fn opening(config: ServerConfig) -> (Self, impl Future<Output = ()> + Send) {
        let (server, backend) = OpeningServer::new(config);
        (
            Self {
                state: State::Opening(server),
            },
            backend,
        )
    }

    /// Attempts to open this server for connections.
    ///
    /// See [`WebTransportServer::opening`].
    ///
    /// # Errors
    ///
    /// Errors if this server is already opening or is opened.
    pub fn open(
        &mut self,
        config: ServerConfig,
    ) -> Result<impl Future<Output = ()> + Send, WebTransportError<C2S, S2C, C>> {
        match self.state {
            State::Closed => {
                let (server, backend) = OpeningServer::new(config);
                self.state = State::Opening(server);
                Ok(backend)
            }
            State::Opening(_) | State::Open(_) => Err(WebTransportError::BackendOpen),
        }
    }

    /// Gets the local socket address of this server if it is open.
    ///
    /// # Errors
    ///
    /// Errors if this server is not open.
    pub fn local_addr(
        &self,
    ) -> Result<Result<SocketAddr, &io::Error>, WebTransportError<C2S, S2C, C>> {
        match &self.state {
            State::Closed | State::Opening(_) => Err(WebTransportError::BackendClosed),
            State::Open(server) => Ok(server.local_addr()),
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

    type Event = ServerEvent<C2S, S2C, C>;

    fn connection_info(&self, client: Self::Client) -> Option<Self::ConnectionInfo> {
        match &self.state {
            State::Closed | State::Opening(_) => None,
            State::Open(server) => server.connection_info(client),
        }
    }

    fn connected_clients(&self) -> impl Iterator<Item = Self::Client> {
        let clients = match &self.state {
            State::Closed | State::Opening(_) => None,
            State::Open(server) => Some(server.clients()),
        };
        clients.into_iter().flat_map(|iter| iter)
    }

    fn send(
        &mut self,
        client: Self::Client,
        msg: impl Into<S2C>,
    ) -> Result<(), WebTransportError<C2S, S2C, C>> {
        match &mut self.state {
            State::Closed | State::Opening(_) => Err(WebTransportError::BackendClosed),
            State::Open(server) => server.send(client, msg),
        }
    }

    fn recv<'a>(&mut self) -> impl Iterator<Item = Self::Event> + 'a {
        match &mut self.state {
            State::Closed => vec![].into_iter(),
            State::Opening(server) => match server.poll() {
                Poll::Pending => vec![].into_iter(),
                Poll::Ready(Ok(server)) => {
                    self.state = State::Open(server);
                    vec![ServerEvent::Opened].into_iter()
                }
                Poll::Ready(Err(cause)) => {
                    self.state = State::Closed;
                    vec![ServerEvent::Closed { cause }].into_iter()
                }
            },
            State::Open(server) => match server.recv() {
                (events, Ok(())) => events.into_iter(),
                (mut events, Err(cause)) => {
                    self.state = State::Closed;
                    events.push(ServerEvent::Closed { cause });
                    events.into_iter()
                }
            },
        }
    }

    fn disconnect(&mut self, client: impl Into<Self::Client>) -> Result<(), Self::Error> {
        match &mut self.state {
            State::Closed | State::Opening(_) => Err(WebTransportError::BackendClosed),
            State::Open(server) => server.disconnect(client),
        }
    }
}

impl<C2S, S2C, C> OpeningServer<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    fn new(config: ServerConfig) -> (Self, impl Future<Output = ()> + Send) {
        let (send_open, recv_open) = oneshot::channel();
        (
            Self { recv_open },
            backend::start::<C2S, S2C, C>(config, send_open),
        )
    }

    fn poll(&mut self) -> Poll<OpenServerResult<C2S, S2C, C>> {
        match self.recv_open.try_recv() {
            Ok(result) => Poll::Ready(result),
            Err(oneshot::error::TryRecvError::Empty) => Poll::Pending,
            Err(oneshot::error::TryRecvError::Closed) => {
                Poll::Ready(Err(WebTransportError::BackendClosed))
            }
        }
    }
}

impl<C2S, S2C, C> OpenServer<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    fn local_addr(&self) -> Result<SocketAddr, &io::Error> {
        self.local_addr.as_ref().map(|addr| *addr)
    }

    fn clients(&self) -> impl Iterator<Item = ClientKey> + '_ {
        self.clients.keys()
    }

    fn connection_info(&self, client: ClientKey) -> Option<EndpointInfo> {
        self.clients.get(client).and_then(|client| match client {
            ClientState::Connected(client) => Some(client.info.clone()),
            _ => None,
        })
    }

    fn send(
        &self,
        client: ClientKey,
        msg: impl Into<S2C>,
    ) -> Result<(), WebTransportError<C2S, S2C, C>> {
        let Some(state) = self.clients.get(client) else {
            return Err(WebTransportError::NoClient(client));
        };
        let ClientState::Connected(state) = state else {
            return Err(WebTransportError::NotConnected(client));
        };

        let msg = msg.into();
        state
            .send_s2c
            .send(msg)
            .map_err(|_| WebTransportError::NotConnected(client))
    }

    fn recv(
        &mut self,
    ) -> (
        Vec<ServerEvent<C2S, S2C, C>>,
        Result<(), WebTransportError<C2S, S2C, C>>,
    ) {
        let mut events = Vec::new();
        loop {
            match self.recv_client.try_recv() {
                Ok(client) => {
                    let client = self.clients.insert(ClientState::Incoming(client));
                    events.push(ServerEvent::Incoming { client });
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    return (events, Err(WebTransportError::BackendClosed));
                }
            }
        }

        let mut to_remove = Vec::new();
        for (client, state) in &mut self.clients {
            recv_client(client, state, &mut events, &mut to_remove);
        }
        for client in to_remove {
            self.clients.remove(client);
        }

        (events, Ok(()))
    }

    fn disconnect(
        &mut self,
        client: impl Into<ClientKey>,
    ) -> Result<(), WebTransportError<C2S, S2C, C>> {
        let client = client.into();
        match self.clients.get_mut(client) {
            Some(client) => {
                *client = ClientState::Disconnected;
                Ok(())
            }
            None => Err(WebTransportError::NoClient(client)),
        }
    }
}

fn recv_client<C2S, S2C, C>(
    client: ClientKey,
    state: &mut ClientState<C2S, S2C, C>,
    events: &mut Vec<ServerEvent<C2S, S2C, C>>,
    to_remove: &mut Vec<ClientKey>,
)
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    match state {
        ClientState::Incoming(incoming) => match incoming.recv_accepted.try_recv() {
            Ok(Ok(accepted)) => {
                events.push(ServerEvent::Accepted {
                    client,
                    authority: accepted.authority.clone(),
                    path: accepted.path.clone(),
                    origin: accepted.origin.clone(),
                    user_agent: accepted.user_agent.clone(),
                });
                *state = ClientState::Accepted(accepted);
            }
            Ok(Err(cause)) => {
                events.push(ServerEvent::Disconnected { client, cause });
                to_remove.push(client);
            }
            Err(oneshot::error::TryRecvError::Empty) => {}
            Err(oneshot::error::TryRecvError::Closed) => {
                events.push(ServerEvent::Disconnected {
                    client,
                    cause: WebTransportError::BackendClosed,
                });
                to_remove.push(client);
            }
        },
        ClientState::Accepted(accepted) => match accepted.recv_connected.try_recv() {
            Ok(Ok(connected)) => {
                events.push(ServerEvent::Connected { client });
                *state = ClientState::Connected(connected);
            }
            Ok(Err(cause)) => {
                events.push(ServerEvent::Disconnected { client, cause });
                to_remove.push(client);
            }
            Err(oneshot::error::TryRecvError::Empty) => {}
            Err(oneshot::error::TryRecvError::Closed) => {
                events.push(ServerEvent::Disconnected {
                    client,
                    cause: WebTransportError::BackendClosed,
                });
                to_remove.push(client);
            }
        },
        ClientState::Connected(connected) => {
            while let Ok(info) = connected.recv_info.try_recv() {
                connected.info = info;
            }

            while let Ok(msg) = connected.recv_c2s.try_recv() {
                events.push(ServerEvent::Recv { client, msg });
            }

            match connected.recv_err.try_recv() {
                Ok(cause) => {
                    events.push(ServerEvent::Disconnected { client, cause });
                    to_remove.push(client);
                }
                Err(oneshot::error::TryRecvError::Empty) => {}
                Err(oneshot::error::TryRecvError::Closed) => {
                    events.push(ServerEvent::Disconnected {
                        client,
                        cause: WebTransportError::BackendClosed,
                    });
                    to_remove.push(client);
                }
            }
        }
        ClientState::Disconnected => {
            events.push(ServerEvent::Disconnected {
                client,
                cause: WebTransportError::ForceDisconnect,
            });
            to_remove.push(client);
        }
    }
}
