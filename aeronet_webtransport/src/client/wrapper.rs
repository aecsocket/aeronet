use std::{future::Future, task::Poll};

use aeronet::{
    client::{ClientState, ClientTransport},
    lane::OnLane,
    message::{TryFromBytes, TryIntoBytes},
    protocol::TransportProtocol,
};
use derivative::Derivative;
use xwt_core::utils::maybe;

use crate::{
    ClientMessageKey, ConnectedClient, ConnectingClient, ConnectionInfo, WebTransportClientConfig,
};

use super::{ClientEvent, WebTransportError};

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub enum WebTransportClient<P> {
    #[derivative(Default)]
    Disconnected,
    Connecting(ConnectingClient<P>),
    Connected(ConnectedClient<P>),
}

impl<P> WebTransportClient<P>
where
    P: TransportProtocol,
    P::C2S: TryIntoBytes + TryFromBytes + OnLane,
    P::S2C: TryIntoBytes + TryFromBytes + OnLane,
{
    pub fn connect_new(
        config: WebTransportClientConfig,
    ) -> (Self, impl Future<Output = ()> + maybe::Send) {
        let (frontend, backend) = ConnectingClient::connect(config);
        (Self::Connecting(frontend), backend)
    }

    pub fn connect(
        &mut self,
        config: WebTransportClientConfig,
    ) -> Result<impl Future<Output = ()> + maybe::Send, WebTransportError<P>> {
        match self {
            Self::Disconnected => {
                let (this, backend) = Self::connect_new(config);
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
    P: TransportProtocol,
    P::C2S: TryIntoBytes + TryFromBytes + OnLane,
    P::S2C: TryIntoBytes + TryFromBytes + OnLane,
{
    type Error = WebTransportError<P>;

    type ConnectingInfo = ();

    type ConnectedInfo = ConnectionInfo;

    type MessageKey = ClientMessageKey;

    fn state(&self) -> ClientState<Self::ConnectingInfo, Self::ConnectedInfo> {
        match self {
            Self::Disconnected => ClientState::Disconnected,
            Self::Connecting(_) => ClientState::Connecting(()),
            Self::Connected(client) => ClientState::Connected(client.connection_info()),
        }
    }

    fn send(&mut self, msg: impl Into<P::C2S>) -> Result<Self::MessageKey, Self::Error> {
        match self {
            Self::Disconnected | Self::Connecting(_) => Err(WebTransportError::<P>::NotConnected),
            Self::Connected(client) => client.send(msg),
        }
    }

    fn poll(&mut self) -> impl Iterator<Item = ClientEvent<P>> {
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
            Self::Connected(client) => match client.poll() {
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
