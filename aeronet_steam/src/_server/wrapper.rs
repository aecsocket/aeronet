use aeronet::{
    client::{ClientKey, ClientState},
    server::{ServerState, ServerTransport},
    OnLane, TransportProtocol, TryAsBytes, TryFromBytes,
};
use derivative::Derivative;
use steamworks::{Manager, ServerManager};

use crate::{OpenServer, RemoteConnectedInfo, RemoteConnectingInfo, SteamServerTransportConfig};

use super::{ServerEvent, SteamTransportError};

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub enum SteamServerTransport<P, M = ServerManager> {
    #[derivative(Default)]
    Closed,
    Open(OpenServer<P, M>),
}

impl<P, M> SteamServerTransport<P, M>
where
    P: TransportProtocol,
    P::C2S: TryAsBytes + TryFromBytes + OnLane,
    P::S2C: TryAsBytes + TryFromBytes + OnLane,
    M: Manager + Send + Sync + 'static,
{
    pub fn open_new(
        steam: &steamworks::Client<M>,
        config: SteamServerTransportConfig,
    ) -> Result<Self, SteamTransportError<P>> {
        OpenServer::open(steam, config).map(Self::Open)
    }

    pub fn open(
        &mut self,
        steam: &steamworks::Client<M>,
        config: SteamServerTransportConfig,
    ) -> Result<(), SteamTransportError<P>> {
        match self {
            Self::Closed => {
                *self = Self::open_new(steam, config)?;
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
    P: TransportProtocol,
    P::C2S: TryAsBytes + TryFromBytes + OnLane,
    P::S2C: TryAsBytes + TryFromBytes + OnLane,
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
