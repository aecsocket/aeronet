use std::{future::Future, task::Poll};

use aeronet::{ChannelProtocol, OnChannel, TransportClient, TryAsBytes, TryFromBytes};
use futures::channel::oneshot;

use crate::{EndpointInfo, WebTransportClient, WebTransportConfig, WebTransportError};

use super::{backend, ConnectedClient, ConnectedClientResult, ConnectingClient, State};

impl<P> WebTransportClient<P>
where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    #[must_use]
    pub fn closed() -> Self {
        Self {
            state: State::Disconnected { forced: false },
        }
    }

    pub fn connecting(
        config: WebTransportConfig,
        url: impl Into<String>,
    ) -> Result<(Self, impl Future<Output = ()>), WebTransportError<P>> {
        let (client, backend) = ConnectingClient::new(config, url)?;
        Ok((
            Self {
                state: State::Connecting(client),
            },
            backend,
        ))
    }

    pub fn connect(
        &mut self,
        config: WebTransportConfig,
        url: impl Into<String>,
    ) -> Result<impl Future<Output = ()>, WebTransportError<P>> {
        match self.state {
            State::Disconnected { .. } => {
                let (client, backend) = ConnectingClient::new(config, url)?;
                self.state = State::Connecting(client);
                Ok(backend)
            }
            State::Connecting(_) | State::Connected(_) => Err(WebTransportError::BackendOpen),
        }
    }
}

type ClientEvent<P> = aeronet::ClientEvent<P, WebTransportClient<P>>;

type ClientState = aeronet::ClientState<EndpointInfo>;

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

    fn send(
        &mut self,
        msg: impl Into<<P as aeronet::TransportProtocol>::C2S>,
    ) -> Result<(), Self::Error> {
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
            State::Connected(client) => match client.recv() {
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
        config: WebTransportConfig,
        url: impl Into<String>,
    ) -> Result<(Self, impl Future<Output = ()>), WebTransportError<P>> {
        let url = url.into();
        let (send_connected, recv_connected) = oneshot::channel();
        Ok((
            Self {
                recv_connected,
                send_event: true,
            },
            backend::start::<P>(config, url, send_connected),
        ))
    }

    fn poll(&mut self) -> Poll<ConnectedClientResult<P>> {
        match self.recv_connected.try_recv() {
            Ok(Some(result)) => Poll::Ready(result),
            Ok(None) => Poll::Pending,
            Err(_) => Poll::Ready(Err(WebTransportError::BackendClosed)),
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
        EndpointInfo
    }

    fn send(&mut self, msg: impl Into<P::C2S>) -> Result<(), WebTransportError<P>> {
        let msg = msg.into();
        self.send_c2s
            .unbounded_send(msg)
            .map_err(|_| WebTransportError::BackendClosed)
    }

    fn recv(&mut self) -> (Vec<ClientEvent<P>>, Result<(), WebTransportError<P>>) {
        let mut events = Vec::new();

        // while let Ok(info) = self.recv_info.try_recv() {
        //     self.info = info;
        // }

        while let Ok(Some(msg)) = self.recv_s2c.try_next() {
            events.push(ClientEvent::Recv { msg });
        }

        match self.recv_err.try_recv() {
            Ok(Some(cause)) => (events, Err(cause)),
            Ok(None) => (events, Ok(())),
            Err(_) => (events, Err(WebTransportError::BackendClosed)),
        }
    }
}
