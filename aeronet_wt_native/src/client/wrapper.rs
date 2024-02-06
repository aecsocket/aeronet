use std::{future::Future, task::Poll};

use aeronet::{
    ClientState, ClientTransport, LaneProtocol, OnLane, TransportProtocol, TryAsBytes, TryFromBytes,
};
use derivative::Derivative;
use wtransport::{endpoint::IntoConnectOptions, ClientConfig};

use crate::{ConnectedClient, ConnectingClient, ConnectionInfo};

use super::{ClientEvent, WebTransportError};

/// [`ClientTransport`] implementation using the WebTransport protocol.
///
/// See the [crate-level docs](crate).
#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub enum WebTransportClient<P: TransportProtocol> {
    /// See [`ClientState::Disconnected`].
    #[derivative(Default)]
    Disconnected,
    /// See [`ClientState::Connecting`].
    Connecting(ConnectingClient<P>),
    /// See [`ClientState::Connected`].
    Connected(ConnectedClient<P>),
}

impl<P> WebTransportClient<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
{
    pub fn connect_new(
        config: ClientConfig,
        options: impl IntoConnectOptions,
    ) -> (Self, impl Future<Output = ()> + Send) {
        let (frontend, backend) = ConnectingClient::connect(config, options);
        (Self::Connecting(frontend), backend)
    }

    pub fn connect(
        &mut self,
        config: ClientConfig,
        options: impl IntoConnectOptions,
    ) -> Result<impl Future<Output = ()> + Send, WebTransportError<P>> {
        match self {
            Self::Disconnected => {
                let (this, backend) = Self::connect_new(config, options);
                *self = this;
                Ok(backend)
            }
            Self::Connecting(_) | Self::Connected(_) => {
                Err(WebTransportError::<P>::AlreadyConnected)
            }
        }
    }

    pub fn disconnect(&mut self) -> Result<(), WebTransportError<P>> {
        match self {
            Self::Disconnected => Err(WebTransportError::<P>::AlreadyDisconnected),
            Self::Connecting(_) | Self::Connected(_) => {
                *self = Self::Disconnected;
                Ok(())
            }
        }
    }
}

impl<P> ClientTransport<P> for WebTransportClient<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
{
    type Error = WebTransportError<P>;

    type ConnectingInfo = ();

    type ConnectedInfo = ConnectionInfo;

    fn state(&self) -> ClientState<Self::ConnectingInfo, Self::ConnectedInfo> {
        match self {
            Self::Disconnected => ClientState::Disconnected,
            Self::Connecting(_) => ClientState::Connecting(()),
            Self::Connected(client) => ClientState::Connected(client.connection_info()),
        }
    }

    fn send(&mut self, msg: impl Into<P::C2S>) -> Result<(), Self::Error> {
        match self {
            Self::Disconnected | Self::Connecting(_) => Err(WebTransportError::<P>::NotConnected),
            Self::Connected(client) => client.send(msg),
        }
    }

    fn update(&mut self) -> impl Iterator<Item = ClientEvent<P>> {
        match self {
            Self::Disconnected => vec![],
            Self::Connecting(client) => match client.poll() {
                Poll::Pending => vec![],
                Poll::Ready(Ok(client)) => {
                    *self = Self::Connected(client);
                    vec![ClientEvent::Connected]
                }
                Poll::Ready(Err(reason)) => {
                    *self = Self::Disconnected;
                    vec![ClientEvent::Disconnected { reason }]
                }
            },
            Self::Connected(client) => match client.update() {
                (events, Ok(())) => events,
                (mut events, Err(reason)) => {
                    events.push(ClientEvent::Disconnected { reason });
                    *self = Self::Disconnected;
                    events
                }
            },
        }
        .into_iter()
    }
}
