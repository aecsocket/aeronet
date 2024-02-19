use std::{net::SocketAddr, task::Poll};

use aeronet::{ClientState, ClientTransport, LaneProtocol, OnLane, TryAsBytes, TryFromBytes};
use derivative::Derivative;
use steamworks::{ClientManager, Manager, SteamId};

use crate::{ConnectedClient, ConnectingClient, ConnectionInfo};

use super::{ClientEvent, SteamTransportError};

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub enum SteamClientTransport<P, M = ClientManager> {
    #[derivative(Default)]
    Disconnected,
    Connecting(ConnectingClient<P, M>),
    Connected(ConnectedClient<P, M>),
}

impl<P, M> SteamClientTransport<P, M>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
    M: Manager + Send + Sync + 'static,
{
    pub fn connect_new_ip(
        steam: steamworks::Client<M>,
        target: SocketAddr,
    ) -> Result<Self, SteamTransportError<P>> {
        ConnectingClient::connect_ip(steam, target).map(Self::Connecting)
    }

    pub fn connect_new_p2p(
        steam: steamworks::Client<M>,
        target: SteamId,
        virtual_port: i32,
    ) -> Result<Self, SteamTransportError<P>> {
        ConnectingClient::connect_p2p(steam, target, virtual_port).map(Self::Connecting)
    }

    pub fn connect_ip(
        &mut self,
        steam: steamworks::Client<M>,
        target: SocketAddr,
    ) -> Result<(), SteamTransportError<P>> {
        match self {
            Self::Disconnected => {
                *self = Self::connect_new_ip(steam, target)?;
                Ok(())
            }
            _ => Err(SteamTransportError::<P>::AlreadyConnected),
        }
    }

    pub fn connect_p2p(
        &mut self,
        steam: steamworks::Client<M>,
        target: SteamId,
        virtual_port: i32,
    ) -> Result<(), SteamTransportError<P>> {
        match self {
            Self::Disconnected => {
                *self = Self::connect_new_p2p(steam, target, virtual_port)?;
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
    P: LaneProtocol,
    P::C2S: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
    M: Manager + Send + Sync + 'static,
{
    type Error = SteamTransportError<P>;

    type ConnectingInfo = ();

    type ConnectedInfo = ConnectionInfo;

    fn state(&self) -> ClientState<Self::ConnectingInfo, Self::ConnectedInfo> {
        match self {
            Self::Disconnected => ClientState::Disconnected,
            Self::Connecting(_) => ClientState::Connecting(()),
            Self::Connected(client) => ClientState::Connected(client.info()),
        }
    }

    fn send(&mut self, msg: impl Into<P::C2S>) -> Result<(), Self::Error> {
        match self {
            Self::Disconnected => Err(SteamTransportError::<P>::NotConnected),
            Self::Connecting(_) => Err(SteamTransportError::<P>::NotConnected),
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
                    *self = Self::Disconnected;
                    events.push(ClientEvent::Disconnected { reason });
                    events
                }
            },
        }
        .into_iter()
    }
}
