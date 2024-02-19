use std::net::SocketAddr;

use aeronet::{
    ClientKey, ClientState, LaneProtocol, OnLane, ServerState, ServerTransport, TransportProtocol,
    TryAsBytes, TryFromBytes,
};
use derivative::Derivative;
use steamworks::{Manager, ServerManager};

use crate::{OpenServer, RemoteConnectedInfo, RemoteConnectingInfo};

use super::{ServerEvent, SteamTransportError};

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub enum SteamServerTransport<P: TransportProtocol, M = ServerManager> {
    #[derivative(Default)]
    Closed,
    Open(OpenServer<P, M>),
}

impl<P, M> SteamServerTransport<P, M>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
    M: Manager + Send + Sync + 'static,
{
    pub fn open_new_ip(
        steam: &steamworks::Client<M>,
        addr: SocketAddr,
    ) -> Result<Self, SteamTransportError<P>> {
        OpenServer::open_ip(steam, addr).map(Self::Open)
    }

    pub fn open_new_p2p(
        steam: &steamworks::Client<M>,
        virtual_port: i32,
    ) -> Result<Self, SteamTransportError<P>> {
        OpenServer::open_p2p(steam, virtual_port).map(Self::Open)
    }

    pub fn open_ip(
        &mut self,
        steam: &steamworks::Client<M>,
        addr: SocketAddr,
    ) -> Result<(), SteamTransportError<P>> {
        match self {
            Self::Closed => {
                *self = Self::open_new_ip(steam, addr)?;
                Ok(())
            }
            Self::Open(_) => Err(SteamTransportError::<P>::AlreadyOpen),
        }
    }

    pub fn open_p2p(
        &mut self,
        steam: &steamworks::Client<M>,
        virtual_port: i32,
    ) -> Result<(), SteamTransportError<P>> {
        match self {
            Self::Closed => {
                *self = Self::open_new_p2p(steam, virtual_port)?;
                Ok(())
            }
            Self::Open(_) => Err(SteamTransportError::<P>::AlreadyOpen),
        }
    }

    pub fn close(&mut self) -> Result<(), SteamTransportError<P>> {
        match self {
            Self::Closed => Err(SteamTransportError::<P>::AlreadyClosed),
            Self::Open(_) => {
                *self = Self::Closed;
                Ok(())
            }
        }
    }

    pub fn accept_request(&mut self, client: ClientKey) -> Result<(), SteamTransportError<P>> {
        match self {
            Self::Closed => Err(SteamTransportError::<P>::NotOpen),
            Self::Open(server) => server.accept_request(client),
        }
    }

    pub fn reject_request(&mut self, client: ClientKey) -> Result<(), SteamTransportError<P>> {
        match self {
            Self::Closed => Err(SteamTransportError::<P>::NotOpen),
            Self::Open(server) => server.reject_request(client),
        }
    }
}

impl<P, M> ServerTransport<P> for SteamServerTransport<P, M>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
    M: Manager + Send + Sync + 'static,
{
    type Error = SteamTransportError<P>;

    type OpeningInfo = ();

    type OpenInfo = ();

    type ConnectingInfo = RemoteConnectingInfo;

    type ConnectedInfo = RemoteConnectedInfo;

    fn state(&self) -> ServerState<(), ()> {
        match self {
            Self::Closed => ServerState::Closed,
            Self::Open(_) => ServerState::Open(()),
        }
    }

    fn client_state(
        &self,
        client: ClientKey,
    ) -> ClientState<Self::ConnectingInfo, Self::ConnectedInfo> {
        match self {
            Self::Closed => ClientState::Disconnected,
            Self::Open(server) => server.client_state(client),
        }
    }

    fn client_keys(&self) -> impl Iterator<Item = ClientKey> + '_ {
        match self {
            Self::Closed => None,
            Self::Open(server) => Some(server.client_keys()),
        }
        .into_iter()
        .flatten()
    }

    fn send(&mut self, client: ClientKey, msg: impl Into<P::S2C>) -> Result<(), Self::Error> {
        match self {
            Self::Closed => Err(SteamTransportError::<P>::NotOpen),
            Self::Open(server) => server.send(client, msg),
        }
    }

    fn disconnect(&mut self, client: ClientKey) -> Result<(), Self::Error> {
        match self {
            Self::Closed => Err(SteamTransportError::<P>::NotOpen),
            Self::Open(server) => server.disconnect(client),
        }
    }

    fn poll(&mut self) -> impl Iterator<Item = ServerEvent<P>> {
        match self {
            Self::Closed => vec![],
            Self::Open(server) => server.poll(),
        }
        .into_iter()
    }
}
