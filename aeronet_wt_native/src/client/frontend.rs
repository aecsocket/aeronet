use std::{future::Future, task::Poll};

use aeronet::{ChannelKey, Message, OnChannel, TransportClient, TryFromBytes, TryIntoBytes};
use tokio::sync::oneshot;
use wtransport::ClientConfig;

use crate::{ClientEvent, EndpointInfo};

use super::{backend, ConnectedClient, ConnectedClientResult, ConnectingClient, WebTransportError};

/// Implementation of [`TransportClient`] using the WebTransport protocol.
///
/// See the [crate-level docs](crate).
#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct WebTransportClient<C2S, S2C, C>
where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: ChannelKey,
{
    state: State<C2S, S2C, C>,
}

#[derive(Debug)]
enum State<C2S, S2C, C>
where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: ChannelKey,
{
    Disconnected,
    Connecting(ConnectingClient<C2S, S2C, C>),
    Connected(ConnectedClient<C2S, S2C, C>),
}

impl<C2S, S2C, C> WebTransportClient<C2S, S2C, C>
where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: ChannelKey,
{
    /// Creates a new client which is not connecting to any server.
    ///
    /// This is useful if you want to prepare a client for connecting, but you
    /// do not have a target server to connect to yet.
    ///
    /// If you want to create a client and connect to a server immediately after
    /// creation, use [`WebTransportClient::connecting`] instead.
    pub fn disconnected() -> Self {
        Self {
            state: State::Disconnected,
        }
    }

    /// Creates and starts connecting a client to a server.
    ///
    /// The URL must have protocol `https://`.
    ///
    /// This returns:
    /// * the client frontend
    ///   * use this throughout your app to interface with the client
    /// * a [`Future`] for the client's backend task
    ///   * run this on an async runtime as soon as possible
    pub fn connecting(
        config: ClientConfig,
        url: impl Into<String>,
    ) -> (Self, impl Future<Output = ()> + Send) {
        let (client, backend) = ConnectingClient::new(config, url);
        (
            Self {
                state: State::Connecting(client),
            },
            backend,
        )
    }

    /// Attempts to start connecting this client to a server.
    ///
    /// See [`WebTransportClient::connecting`].
    ///
    /// # Errors
    ///
    /// Errors if this client is already connecting or is connected to a server.
    pub fn connect(
        &mut self,
        config: ClientConfig,
        url: impl Into<String>,
    ) -> Result<impl Future<Output = ()> + Send, WebTransportError<C2S, S2C, C>> {
        match self.state {
            State::Disconnected => {
                let (client, backend) = ConnectingClient::new(config, url);
                self.state = State::Connecting(client);
                Ok(backend)
            }
            State::Connecting(_) | State::Connected(_) => Err(WebTransportError::BackendOpen),
        }
    }
}

impl<C2S, S2C, C> TransportClient<C2S, S2C> for WebTransportClient<C2S, S2C, C>
where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: ChannelKey,
{
    type Error = WebTransportError<C2S, S2C, C>;

    type ConnectionInfo = EndpointInfo;

    type Event = ClientEvent<C2S, S2C, C>;

    fn connection_info(&self) -> Option<Self::ConnectionInfo> {
        match &self.state {
            State::Disconnected | State::Connecting(_) => None,
            State::Connected(client) => client.connection_info(),
        }
    }

    fn send(&mut self, msg: impl Into<C2S>) -> Result<(), Self::Error> {
        match &mut self.state {
            State::Disconnected | State::Connecting(_) => Err(WebTransportError::BackendClosed),
            State::Connected(client) => client.send(msg),
        }
    }

    fn recv<'a>(&mut self) -> impl Iterator<Item = Self::Event> + 'a {
        match &mut self.state {
            State::Disconnected => vec![].into_iter(),
            State::Connecting(client) => match client.poll() {
                Poll::Pending => vec![].into_iter(),
                Poll::Ready(Ok(client)) => {
                    self.state = State::Connected(client);
                    vec![ClientEvent::Connected].into_iter()
                }
                Poll::Ready(Err(cause)) => {
                    self.state = State::Disconnected;
                    vec![ClientEvent::Disconnected { cause }].into_iter()
                }
            },
            State::Connected(server) => match server.recv() {
                (events, Ok(())) => events.into_iter(),
                (mut events, Err(cause)) => {
                    self.state = State::Disconnected;
                    events.push(ClientEvent::Disconnected { cause });
                    events.into_iter()
                }
            },
        }
    }

    fn disconnect(&mut self) -> Result<(), Self::Error> {
        match self.state {
            State::Disconnected | State::Connecting(_) => Err(WebTransportError::BackendClosed),
            State::Connected(_) => {
                self.state = State::Disconnected;
                Ok(())
            }
        }
    }
}

impl<C2S, S2C, C> ConnectingClient<C2S, S2C, C>
where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: ChannelKey,
{
    fn new(
        config: ClientConfig,
        url: impl Into<String>,
    ) -> (Self, impl Future<Output = ()> + Send) {
        let (send_connected, recv_connected) = oneshot::channel();
        let url = url.into();
        (
            Self { recv_connected },
            backend::start::<C2S, S2C, C>(config, url, send_connected),
        )
    }

    fn poll(&mut self) -> Poll<ConnectedClientResult<C2S, S2C, C>> {
        match self.recv_connected.try_recv() {
            Ok(result) => Poll::Ready(result),
            Err(oneshot::error::TryRecvError::Empty) => Poll::Pending,
            Err(oneshot::error::TryRecvError::Closed) => {
                Poll::Ready(Err(WebTransportError::BackendClosed))
            }
        }
    }
}

impl<C2S, S2C, C> ConnectedClient<C2S, S2C, C>
where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: ChannelKey,
{
    fn connection_info(&self) -> Option<EndpointInfo> {
        Some(self.info.clone())
    }

    fn send(&mut self, msg: impl Into<C2S>) -> Result<(), WebTransportError<C2S, S2C, C>> {
        let msg = msg.into();
        self.send_c2s
            .send(msg)
            .map_err(|_| WebTransportError::BackendClosed)
    }

    fn recv(
        &mut self,
    ) -> (
        Vec<ClientEvent<C2S, S2C, C>>,
        Result<(), WebTransportError<C2S, S2C, C>>,
    ) {
        let mut events = Vec::new();

        while let Ok(info) = self.recv_info.try_recv() {
            self.info = info;
        }

        while let Ok(msg) = self.recv_s2c.try_recv() {
            events.push(ClientEvent::Recv { msg });
        }

        match self.recv_err.try_recv() {
            Ok(cause) => (events, Err(cause)),
            Err(oneshot::error::TryRecvError::Empty) => (events, Ok(())),
            Err(oneshot::error::TryRecvError::Closed) => (events, Err(WebTransportError::BackendClosed)),
        }
    }
}
