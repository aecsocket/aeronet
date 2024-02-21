use std::task::Poll;

use aeronet::{
    client::{ClientState, ClientTransport},
    OnLane, TransportProtocol, TryAsBytes, TryFromBytes,
};
use derivative::Derivative;
use steamworks::{ClientManager, Manager};

use crate::{
    ConnectedClient, ConnectingClient, ConnectionInfo, NegotiatingClient,
    SteamClientTransportConfig,
};

use super::{ClientEvent, SteamTransportError};

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub enum SteamClientTransport<P, M = ClientManager> {
    #[derivative(Default)]
    Disconnected,
    Connecting(ConnectingClient<P, M>),
    Negotiating(NegotiatingClient<P, M>),
    Connected(ConnectedClient<P, M>),
}

impl<P, M> SteamClientTransport<P, M>
where
    P: TransportProtocol,
    P::C2S: TryAsBytes + TryFromBytes + OnLane,
    P::S2C: TryAsBytes + TryFromBytes + OnLane,
    M: Manager + Send + Sync + 'static,
{
    pub fn connect_new(
        steam: steamworks::Client<M>,
        config: SteamClientTransportConfig,
    ) -> Result<Self, SteamTransportError<P>> {
        ConnectingClient::connect(steam, config).map(Self::Connecting)
    }

    pub fn connect(
        &mut self,
        steam: steamworks::Client<M>,
        config: SteamClientTransportConfig,
    ) -> Result<(), SteamTransportError<P>> {
        match self {
            Self::Disconnected => {
                *self = Self::connect_new(steam, config)?;
                Ok(())
            }
            _ => Err(SteamTransportError::<P>::AlreadyConnected),
        }
    }

    pub fn disconnect(&mut self) -> Result<(), SteamTransportError<P>> {
        match self {
            Self::Disconnected => Err(SteamTransportError::<P>::AlreadyDisconnected),
            _ => {
                *self = Self::Disconnected;
                Ok(())
            }
        }
    }
}

impl<P, M> ClientTransport<P> for SteamClientTransport<P, M>
where
    P: TransportProtocol,
    P::C2S: TryAsBytes + TryFromBytes + OnLane,
    P::S2C: TryAsBytes + TryFromBytes + OnLane,
    M: Manager + Send + Sync + 'static,
{
    type Error = SteamTransportError<P>;

    type ConnectingInfo = ();

    type ConnectedInfo = ConnectionInfo;

    fn state(&self) -> ClientState<Self::ConnectingInfo, Self::ConnectedInfo> {
        match self {
            Self::Disconnected => ClientState::Disconnected,
            Self::Connecting(_) | Self::Negotiating(_) => ClientState::Connecting(()),
            Self::Connected(client) => ClientState::Connected(client.info()),
        }
    }

    fn send(&mut self, msg: impl Into<P::C2S>) -> Result<(), Self::Error> {
        match self {
            Self::Disconnected | Self::Connecting(_) | Self::Negotiating(_) => {
                Err(SteamTransportError::<P>::NotConnected)
            }
            Self::Connected(client) => client.send(msg),
        }
    }

    fn poll(&mut self) -> impl Iterator<Item = ClientEvent<P>> {
        match self {
            Self::Disconnected => vec![],
            Self::Connecting(client) => match client.poll() {
                Poll::Pending => vec![],
                Poll::Ready(Ok(client)) => {
                    *self = Self::Negotiating(client);
                    vec![]
                }
                Poll::Ready(Err(reason)) => {
                    *self = Self::Disconnected;
                    vec![ClientEvent::Disconnected { reason }]
                }
            },
            Self::Negotiating(client) => match client.poll() {
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
                    *self = Self::Disconnected;
                    events.push(ClientEvent::Disconnected { reason });
                    events
                }
            },
        }
        .into_iter()
    }
}
