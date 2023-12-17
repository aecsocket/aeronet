use std::{future::Future, task::Poll};

use aeronet::{ChannelProtocol, OnChannel, TransportClient, TryAsBytes, TryFromBytes};
use tokio::sync::oneshot;
use wtransport::ClientConfig;

use crate::{shared::ClientState, EndpointInfo, WebTransportClient};

use super::{
    backend, ClientEvent, ConnectedClient, ConnectedClientResult, ConnectingClient, State,
    WebTransportError,
};

impl<P> WebTransportClient<P>
where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
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
            state: State::Disconnected { forced: false },
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
            State::Disconnected { .. } => {
                let (client, backend) = ConnectingClient::new(config, url);
                self.state = State::Connecting(client);
                Ok(backend)
            }
            State::Connecting(_) | State::Connected(_) => Err(WebTransportError::BackendOpen),
        }
    }
}

impl<P> TransportClient<P> for WebTransportClient<P>
where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    type Error = WebTransportError<P>;

    type ConnectionInfo = EndpointInfo;

    type Event = ClientEvent<P>;

    fn state(&self) -> ClientState {
        match &self.state {
            State::Disconnected { .. } => ClientState::Disconnected,
            State::Connecting(_) => ClientState::Connecting,
            State::Connected(client) => ClientState::Connected(client.connection_info()),
        }
    }

    fn send(&mut self, msg: impl Into<P::C2S>) -> Result<(), Self::Error> {
        match &mut self.state {
            State::Disconnected { .. } | State::Connecting(_) => {
                Err(WebTransportError::BackendClosed)
            }
            State::Connected(client) => client.send(msg),
        }
    }

    fn recv<'a>(&mut self) -> impl Iterator<Item = Self::Event> + 'a {
        match &mut self.state {
            State::Disconnected { forced } => {
                if *forced {
                    *forced = false;
                    vec![ClientEvent::Disconnected {
                        cause: WebTransportError::ForceDisconnect,
                    }]
                } else {
                    vec![]
                }
            }
            State::Connecting(client) => {
                let mut events = Vec::new();

                if client.send_event {
                    client.send_event = false;
                    events.push(ClientEvent::Connecting);
                }

                match client.poll() {
                    Poll::Pending => {}
                    Poll::Ready(Ok(client)) => {
                        self.state = State::Connected(client);
                        events.push(ClientEvent::Connected);
                    }
                    Poll::Ready(Err(cause)) => {
                        self.state = State::Disconnected { forced: false };
                        events.push(ClientEvent::Disconnected { cause });
                    }
                }

                events
            }
            State::Connected(server) => match server.recv() {
                (events, Ok(())) => events,
                (mut events, Err(cause)) => {
                    self.state = State::Disconnected { forced: false };
                    events.push(ClientEvent::Disconnected { cause });
                    events
                }
            },
        }
        .into_iter()
    }

    fn disconnect(&mut self) -> Result<(), Self::Error> {
        match self.state {
            State::Disconnected { .. } => Err(WebTransportError::BackendClosed),
            State::Connecting(_) | State::Connected(_) => {
                self.state = State::Disconnected { forced: true };
                Ok(())
            }
        }
    }
}

impl<P> ConnectingClient<P>
where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    fn new(
        config: ClientConfig,
        url: impl Into<String>,
    ) -> (Self, impl Future<Output = ()> + Send) {
        let (send_connected, recv_connected) = oneshot::channel();
        let url = url.into();
        (
            Self {
                recv_connected,
                send_event: true,
            },
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
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
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
