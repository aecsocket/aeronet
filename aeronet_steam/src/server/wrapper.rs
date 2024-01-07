use std::net::SocketAddr;

use aeronet::{LaneProtocol, TryFromBytes, TryAsBytes, OnLane, ClientKey, ServerTransport};
use derivative::Derivative;
use steamworks::{ServerManager, Manager};

use crate::{OpenServer, RemoteConnectingInfo, RemoteConnectedInfo};

use super::{SteamTransportError, ServerState, ClientState, ServerEvent};

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub enum SteamServerTransport<P, M = ServerManager>
where
    P: LaneProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnLane<Lane = P::Lane>,
    M: Manager + Send + Sync + 'static,
{
    #[derivative(Default)]
    Closed,
    Open(OpenServer<P, M>),
}

// impl<P, M> bevy::prelude::Resource for SteamServerTransport<P, M>
// where
//     P: LaneProtocol,
//     P::C2S: TryFromBytes,
//     P::S2C: TryAsBytes + OnLane<Lane = P::Lane>,
//     M: Manager + Send + Sync + 'static,
// {
// }

impl<P, M> SteamServerTransport<P, M>
where
    P: LaneProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnLane<Lane = P::Lane>,
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
        port: i32,
    ) -> Result<Self, SteamTransportError<P>> {
        OpenServer::open_p2p(steam, port).map(Self::Open)
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
        port: i32,
    ) -> Result<(), SteamTransportError<P>> {
        match self {
            Self::Closed => {
                *self = Self::open_new_p2p(steam, port)?;
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

    pub fn accept_client(&mut self, client: ClientKey) -> Result<(), SteamTransportError<P>> {
        match self {
            Self::Closed => Err(SteamTransportError::<P>::NotOpen),
            Self::Open(server) => server.accept_client(client),
        }
    }

    pub fn reject_client(&mut self, client: ClientKey) -> Result<(), SteamTransportError<P>> {
        match self {
            Self::Closed => Err(SteamTransportError::<P>::NotOpen),
            Self::Open(server) => server.reject_client(client),
        }
    }
}

impl<P, M> ServerTransport<P> for SteamServerTransport<P, M>
where
    P: LaneProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnLane<Lane = P::Lane>,
    M: Manager + Send + Sync + 'static,
{
    type Error = SteamTransportError<P>;

    type OpeningInfo = ();

    type OpenInfo = ();

    type ConnectingInfo = RemoteConnectingInfo;

    type ConnectedInfo = RemoteConnectedInfo;

    fn state(&self) -> ServerState {
        match self {
            Self::Closed => ServerState::Closed,
            Self::Open(_) => ServerState::Open(()),
        }
    }

    fn client_state(&self, client: ClientKey) -> ClientState {
        match self {
            Self::Closed => ClientState::Disconnected,
            Self::Open(server) => server.client_state(client),
        }
    }

    fn clients(&self) -> impl Iterator<Item = ClientKey> {
        match self {
            Self::Closed => None,
            Self::Open(server) => Some(server.clients()),
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

    fn update(&mut self) -> impl Iterator<Item = ServerEvent<P>> {
        match self {
            Self::Closed => vec![],
            Self::Open(server) => server.update(),
        }
        .into_iter()
    }
}
