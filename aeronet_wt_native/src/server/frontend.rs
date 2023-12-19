use std::{future::Future, io, net::SocketAddr, task::Poll};

use aeronet::{ChannelProtocol, OnChannel, TransportServer, TryAsBytes, TryFromBytes};
use tokio::sync::{mpsc, oneshot};
use tracing::debug;
use wtransport::ServerConfig;

use crate::{shared::ClientState, ClientKey, EndpointInfo, ServerEvent, WebTransportServer};

use super::{
    backend, OpenServer, OpenServerResult, OpeningServer, RemoteClient, State, WebTransportError,
};

impl<P> WebTransportServer<P>
where
    P: ChannelProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnChannel<Channel = P::Channel>,
{
    /// Creates a new server which is not open for connections, and is not
    /// starting to open.
    ///
    /// This is useful if you want to prepare a server, but do not want to open
    /// it up to accepting connections yet.
    ///
    /// If you want to create a server and start listening for connections
    /// immediately after creation, use [`WebTransportServer::opening`] instead.
    #[must_use]
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
    ) -> Result<impl Future<Output = ()> + Send, WebTransportError<P>> {
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
    pub fn local_addr(&self) -> Result<Result<SocketAddr, &io::Error>, WebTransportError<P>> {
        match &self.state {
            State::Closed | State::Opening(_) => Err(WebTransportError::BackendClosed),
            State::Open(server) => Ok(server.local_addr()),
        }
    }
}

impl<P> TransportServer<P> for WebTransportServer<P>
where
    P: ChannelProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnChannel<Channel = P::Channel>,
{
    type Client = ClientKey;

    type Error = WebTransportError<P>;

    type ConnectionInfo = EndpointInfo;

    type Event = ServerEvent<P>;

    fn client_state(&self, client: Self::Client) -> ClientState {
        match &self.state {
            State::Closed | State::Opening(_) => ClientState::Disconnected,
            State::Open(server) => server.client_state(client),
        }
    }

    fn clients(&self) -> impl Iterator<Item = (Self::Client, ClientState)> {
        let clients = match &self.state {
            State::Closed | State::Opening(_) => None,
            State::Open(server) => Some(server.clients()),
        };
        clients.into_iter().flatten()
    }

    fn send(
        &mut self,
        client: Self::Client,
        msg: impl Into<P::S2C>,
    ) -> Result<(), WebTransportError<P>> {
        match &mut self.state {
            State::Closed | State::Opening(_) => Err(WebTransportError::BackendClosed),
            State::Open(server) => server.send(client, msg),
        }
    }

    fn recv<'a>(&mut self) -> impl Iterator<Item = Self::Event> + 'a {
        match &mut self.state {
            State::Closed => vec![],
            State::Opening(server) => match server.poll() {
                Poll::Pending => vec![],
                Poll::Ready(Ok(server)) => {
                    self.state = State::Open(server);
                    vec![ServerEvent::Opened]
                }
                Poll::Ready(Err(cause)) => {
                    self.state = State::Closed;
                    vec![ServerEvent::Closed { cause }]
                }
            },
            State::Open(server) => match server.recv() {
                (events, Ok(())) => events,
                (mut events, Err(cause)) => {
                    self.state = State::Closed;
                    events.push(ServerEvent::Closed { cause });
                    events
                }
            },
        }
        .into_iter()
    }

    fn disconnect(&mut self, client: impl Into<Self::Client>) -> Result<(), Self::Error> {
        match &mut self.state {
            State::Closed | State::Opening(_) => Err(WebTransportError::BackendClosed),
            State::Open(server) => server.disconnect(client),
        }
    }
}

impl<P> OpeningServer<P>
where
    P: ChannelProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnChannel<Channel = P::Channel>,
{
    fn new(config: ServerConfig) -> (Self, impl Future<Output = ()> + Send) {
        let (send_open, recv_open) = oneshot::channel();
        (Self { recv_open }, backend::start::<P>(config, send_open))
    }

    fn poll(&mut self) -> Poll<OpenServerResult<P>> {
        match self.recv_open.try_recv() {
            Ok(result) => Poll::Ready(result),
            Err(oneshot::error::TryRecvError::Empty) => Poll::Pending,
            Err(oneshot::error::TryRecvError::Closed) => {
                Poll::Ready(Err(WebTransportError::BackendClosed))
            }
        }
    }
}

impl<P> OpenServer<P>
where
    P: ChannelProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnChannel<Channel = P::Channel>,
{
    fn local_addr(&self) -> Result<SocketAddr, &io::Error> {
        self.local_addr.as_ref().map(|addr| *addr)
    }

    fn client_state(&self, client: ClientKey) -> ClientState {
        match self.clients.get(client) {
            None | Some(RemoteClient::Disconnected) => ClientState::Disconnected,
            Some(
                RemoteClient::Untracked(_) | RemoteClient::Incoming(_) | RemoteClient::Accepted(_),
            ) => ClientState::Connecting,
            Some(RemoteClient::Connected(client)) => ClientState::Connected(client.info.clone()),
        }
    }

    fn clients(&self) -> impl Iterator<Item = (ClientKey, ClientState)> + '_ {
        self.clients
            .keys()
            .map(|client| (client, self.client_state(client)))
    }

    fn send(&self, client: ClientKey, msg: impl Into<P::S2C>) -> Result<(), WebTransportError<P>> {
        let Some(state) = self.clients.get(client) else {
            return Err(WebTransportError::NoClient(client));
        };
        let RemoteClient::Connected(state) = state else {
            return Err(WebTransportError::NotConnected(client));
        };

        let msg = msg.into();
        state
            .send_s2c
            .send(msg)
            .map_err(|_| WebTransportError::NotConnected(client))
    }

    fn recv(&mut self) -> (Vec<ServerEvent<P>>, Result<(), WebTransportError<P>>) {
        let mut events = Vec::new();
        loop {
            match self.recv_client.try_recv() {
                Ok(client) => {
                    let client = self.clients.insert(RemoteClient::Untracked(client));
                    debug!("Inserted client {client:?}");
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
            debug!("Removed {client:?}");
        }

        (events, Ok(()))
    }

    fn disconnect(&mut self, client: impl Into<ClientKey>) -> Result<(), WebTransportError<P>> {
        let client = client.into();
        match self.clients.get_mut(client) {
            Some(client) => {
                *client = RemoteClient::Disconnected;
                Ok(())
            }
            None => Err(WebTransportError::NoClient(client)),
        }
    }
}

fn recv_client<P>(
    client: ClientKey,
    state: &mut RemoteClient<P>,
    events: &mut Vec<ServerEvent<P>>,
    to_remove: &mut Vec<ClientKey>,
) where
    P: ChannelProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnChannel<Channel = P::Channel>,
{
    match state {
        RemoteClient::Untracked(untracked) => {
            if let Some(send_key) = untracked.send_key.take() {
                let _ = send_key.send(client);
            }

            match untracked.recv_incoming.try_recv() {
                Ok(incoming) => {
                    *state = RemoteClient::Incoming(incoming);
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
        RemoteClient::Incoming(incoming) => match incoming.recv_accepted.try_recv() {
            Ok(Ok(accepted)) => {
                events.push(ServerEvent::Accepted {
                    client,
                    authority: accepted.authority.clone(),
                    path: accepted.path.clone(),
                    origin: accepted.origin.clone(),
                    user_agent: accepted.user_agent.clone(),
                });
                *state = RemoteClient::Accepted(accepted);
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
        RemoteClient::Accepted(accepted) => match accepted.recv_connected.try_recv() {
            Ok(Ok(connected)) => {
                events.push(ServerEvent::Connected { client });
                *state = RemoteClient::Connected(connected);
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
        RemoteClient::Connected(connected) => {
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
        RemoteClient::Disconnected => {
            events.push(ServerEvent::Disconnected {
                client,
                cause: WebTransportError::ForceDisconnect,
            });
            to_remove.push(client);
        }
    }
}
