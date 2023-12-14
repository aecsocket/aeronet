use std::{future::Future, task::Poll};

use aeronet::{OnChannel, TransportClient, TryFromBytes, TryIntoBytes};
use tokio::sync::oneshot;
use wtransport::ClientConfig;

use crate::{ClientEvent, ClientState, EndpointInfo, WebTransportClient, WebTransportProtocol};

use super::{
    backend, ConnectedClient, ConnectedClientResult, ConnectingClient, State, WebTransportError,
};

impl<P> WebTransportClient<P>
where
    P: WebTransportProtocol,
    P::C2S: TryIntoBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    /// Creates a new client which is not connecting to any server.
    ///
    /// This is useful if you want to prepare a client for connecting, but you
    /// do not have a target server to connect to yet.
    ///
    /// If you want to create a client and connect to a server immediately after
    /// creation, use [`WebTransportClient::connecting`] instead.
    #[must_use]
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
    ) -> Result<impl Future<Output = ()> + Send, WebTransportError<P>> {
        match self.state {
            State::Disconnected => {
                let (client, backend) = ConnectingClient::new(config, url);
                self.state = State::Connecting(client);
                Ok(backend)
            }
            State::Connecting(_) | State::Connected(_) => Err(WebTransportError::BackendOpen),
        }
    }

    /// Gets the current state of the client.
    #[must_use]
    pub fn state(&self) -> ClientState {
        match self.state {
            State::Disconnected => ClientState::Disconnected,
            State::Connecting(_) => ClientState::Connecting,
            State::Connected(_) => ClientState::Connected,
        }
    }
}

impl<P> TransportClient<P> for WebTransportClient<P>
where
    P: WebTransportProtocol,
    P::C2S: TryIntoBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    type Error = WebTransportError<P>;

    type ConnectionInfo = EndpointInfo;

    type Event = ClientEvent<P>;

    fn connection_info(&self) -> Option<Self::ConnectionInfo> {
        match &self.state {
            State::Disconnected | State::Connecting(_) => None,
            State::Connected(client) => Some(client.connection_info()),
        }
    }

    fn send(&mut self, msg: impl Into<P::C2S>) -> Result<(), Self::Error> {
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

impl<P> ConnectingClient<P>
where
    P: WebTransportProtocol,
    P::C2S: TryIntoBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    fn new(
        config: ClientConfig,
        url: impl Into<String>,
    ) -> (Self, impl Future<Output = ()> + Send) {
        let (send_connected, recv_connected) = oneshot::channel();
        let url = url.into();
        (
            Self { recv_connected },
            backend::start::<P>(config, url, send_connected),
        )
    }

    fn poll(&mut self) -> Poll<ConnectedClientResult<P>> {
        match self.recv_connected.try_recv() {
            Ok(result) => Poll::Ready(result),
            Err(oneshot::error::TryRecvError::Empty) => Poll::Pending,
            Err(oneshot::error::TryRecvError::Closed) => {
                Poll::Ready(Err(WebTransportError::BackendClosed))
            }
        }
    }
}

impl<P> ConnectedClient<P>
where
    P: WebTransportProtocol,
    P::C2S: TryIntoBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    fn connection_info(&self) -> EndpointInfo {
        self.info.clone()
    }

    fn send(&mut self, msg: impl Into<P::C2S>) -> Result<(), WebTransportError<P>> {
        let msg = msg.into();
        self.send_c2s
            .send(msg)
            .map_err(|_| WebTransportError::BackendClosed)
    }

    fn recv(&mut self) -> (Vec<ClientEvent<P>>, Result<(), WebTransportError<P>>) {
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
            Err(oneshot::error::TryRecvError::Closed) => {
                (events, Err(WebTransportError::BackendClosed))
            }
        }
    }
}
