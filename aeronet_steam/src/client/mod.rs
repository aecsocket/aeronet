mod inner;

pub use inner::*;

use aeronet::{
    ClientTransport, LaneProtocol, MessageState, MessageTicket, OnLane, TransportConfig,
    TransportProtocol, TryAsBytes, TryFromBytes,
};
use steamworks::{networking_types::NetworkingIdentity, ClientManager};

type ClientState = aeronet::ClientState<()>;

type ClientEvent<P> = aeronet::ClientEvent<P, SteamClientTransport<P>>;

type SteamTransportError<P> = crate::SteamTransportError<
    <P as TransportProtocol>::C2S,
    <P as TransportProtocol>::S2C,
    <P as LaneProtocol>::Lane,
>;

type LaneError<P> = crate::LaneError<<P as TransportProtocol>::C2S, <P as TransportProtocol>::S2C>;

#[derive(Default)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub enum SteamClientTransport<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    #[default]
    Disconnected,
    Working(WorkingClient<P>),
}

impl<P> SteamClientTransport<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    pub fn connect_new(
        steam: &steamworks::Client<ClientManager>,
        target: NetworkingIdentity,
        config: TransportConfig,
    ) -> Result<Self, SteamTransportError<P>> {
        let client = WorkingClient::new(steam, target, config)?;
        Ok(Self::Working(client))
    }

    pub fn connect(
        &mut self,
        steam: &steamworks::Client<ClientManager>,
        target: NetworkingIdentity,
        config: TransportConfig,
    ) -> Result<(), SteamTransportError<P>> {
        match self {
            Self::Disconnected => {
                *self = Self::connect_new(steam, target, config)?;
                Ok(())
            }
            Self::Working(_) => Err(SteamTransportError::<P>::AlreadyConnected),
        }
    }

    pub fn disconnect(&mut self) -> Result<(), SteamTransportError<P>> {
        match self {
            Self::Disconnected => Err(SteamTransportError::<P>::AlreadyDisconnected),
            Self::Working(_) => {
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

    type ClientInfo = ();

    fn client_state(&self) -> ClientState {
        match self {
            Self::Disconnected => ClientState::Disconnected,
            Self::Working(client) => client.client_state(),
        }
    }

    fn message_state(&self, msg: MessageTicket) -> MessageState {
        match self {
            Self::Disconnected => MessageState::Unsent,
            Self::Working(client) => client.message_state(msg),
        }
    }

    fn send(
        &self,
        msg: impl Into<<P as aeronet::TransportProtocol>::C2S>,
    ) -> Result<MessageTicket, Self::Error> {
        match self {
            Self::Disconnected => Err(SteamTransportError::<P>::NotConnected),
            Self::Working(client) => client.send(msg),
        }
    }

    fn update(&mut self) -> impl Iterator<Item = ClientEvent<P>> {
        match self {
            Self::Disconnected => vec![],
            Self::Working(client) => match client.recv() {
                Ok(events) => events,
                Err(reason) => {
                    *self = Self::Disconnected;
                    vec![ClientEvent::Disconnected { reason }]
                }
            },
        }
        .into_iter()
    }
}
