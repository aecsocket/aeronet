use std::{net::SocketAddr, task::Poll};

use aeronet::{
    ClientTransport, LaneProtocol, OnLane, TryAsBytes,
    TryFromBytes,
};
use derivative::Derivative;
use steamworks::{ClientManager, Manager, SteamId};

use crate::{ConnectingClient, ConnectedClient, ConnectionInfo};

use super::{SteamTransportError, ClientState, ClientEvent};

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub enum SteamClientTransport<P, M = ClientManager>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
    M: Manager + Send + Sync + 'static,
{
    #[derivative(Default)]
    Disconnected,
    Connecting(ConnectingClient<P, M>),
    Connected(ConnectedClient<P, M>),
}

impl<P, M> SteamClientTransport<P, M>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
    M: Manager + Send + Sync + 'static,
{
    pub fn connect_new_ip(
        steam: steamworks::Client<M>,
        remote: SocketAddr,
    ) -> Result<Self, SteamTransportError<P>> {
        ConnectingClient::connect_ip(steam, remote).map(Self::Connecting)
    }

    pub fn connect_new_p2p(
        steam: steamworks::Client<M>,
        remote: SteamId,
        port: i32,
    ) -> Result<Self, SteamTransportError<P>> {
        ConnectingClient::connect_p2p(steam, remote, port).map(Self::Connecting)
    }

    pub fn connect_ip(
        &mut self,
        steam: steamworks::Client<M>,
        remote: SocketAddr,
    ) -> Result<(), SteamTransportError<P>> {
        match self {
            Self::Disconnected => {
                *self = Self::connect_new_ip(steam, remote)?;
                Ok(())
            }
            _ => Err(SteamTransportError::<P>::AlreadyConnected),
        }
    }

    pub fn connect_p2p(
        &mut self,
        steam: steamworks::Client<M>,
        remote: SteamId,
        port: i32,
    ) -> Result<(), SteamTransportError<P>> {
        match self {
            Self::Disconnected => {
                *self = Self::connect_new_p2p(steam, remote, port)?;
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

impl<P> ClientTransport<P> for SteamClientTransport<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    type Error = SteamTransportError<P>;

    type ConnectingInfo = ();

    type ConnectedInfo = ConnectionInfo;

    fn state(&self) -> ClientState {
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

    fn update(&mut self) -> impl Iterator<Item = ClientEvent<P>> {
        match self {
            Self::Disconnected => vec![],
            Self::Connecting(client) => match client.poll() {
                Poll::Pending => vec![],
                Poll::Ready(Ok(client)) => {
                    let event = ClientEvent::Connected {
                        info: client.info(),
                    };
                    *self = Self::Connected(client);
                    vec![event]
                }
                Poll::Ready(Err(reason)) => {
                    *self = Self::Disconnected;
                    vec![ClientEvent::Disconnected { reason }]
                }
            },
            Self::Connected(client) => match client.update() {
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
