use std::{marker::PhantomData, net::SocketAddr};

use aeronet::{
    ClientState, ClientTransport, LaneKey, LaneKind, LaneProtocol, MessageState, MessageTicket,
    OnLane, TryAsBytes, TryFromBytes,
};
use steamworks::{
    networking_types::{NetworkingIdentity, SendFlags},
    ClientManager, SteamId,
};

use crate::{ClientInfo, SteamClientTransport, SteamTransportError, WorkingClient};

use super::ClientEvent;

// TODO Note on drop impl:
// There already exists a Drop impl for `NetConnection`, sending the message
// "Handle was closed" on drop. Some more customisation would be nice, and
// probably `NetConnection::close` could take `&mut self` instead of `self`.

impl<P> WorkingClient<P>
where
    P: LaneProtocol,
    P::Send: TryAsBytes + OnLane<Lane = P::Lane>,
    P::Recv: TryFromBytes,
{
    pub fn connect_to_peer(
        steam: &steamworks::Client<ClientManager>,
        remote: SteamId,
    ) -> Result<Self, SteamTransportError<P>> {
        let net = steam
            .networking_sockets()
            .connect_p2p(NetworkingIdentity::new_steam_id(remote), 0, Vec::new())
            .map_err(SteamTransportError::Connect)?;

        Ok(Self {
            net,
            _phantom_p: PhantomData::default(),
        })
    }

    pub fn connect_to_server(
        steam: &steamworks::Client<ClientManager>,
        remote: SocketAddr,
    ) -> Result<Self, SteamTransportError<P>> {
        let conn = steam
            .networking_sockets()
            .connect_by_ip_address(remote, Vec::new())
            .map_err(SteamTransportError::Connect)?;

        let num_lanes =
            u8::try_from(P::Lane::VARIANTS.len()).expect("there should be less than 256 lanes");
        let priorities = P::Lane::VARIANTS
            .iter()
            .map(|lane| lane.priority())
            .collect::<Vec<_>>();
        let weights = P::Lane::VARIANTS.iter().map(|_| 0).collect::<Vec<_>>();

        steam.networking_sockets().configure_connection_lanes(
            &conn,
            i32::from(num_lanes),
            &priorities,
            &weights,
        )
        .map_err(SteamTransportError::ConfigureLanes)?;

        Ok(Self {
            net: conn,
            _phantom_p: PhantomData::default(),
        })
    }

    pub fn client_state(&self) -> ClientState<ClientInfo> {
        ClientState::Connecting
    }

    pub fn message_state(&self, msg: MessageTicket) -> MessageState {
        todo!()
    }

    pub fn send(&self, msg: impl Into<P::Send>) -> Result<MessageTicket, SteamTransportError<P>> {
        let msg = msg.into();
        let lane = msg.lane();

        let buf = msg.try_as_bytes().map_err(SteamTransportError::Serialize)?;
        let buf = buf.as_ref();

        let send_flags = match lane.kind() {
            LaneKind::UnreliableUnsequenced | LaneKind::UnreliableSequenced => {
                SendFlags::UNRELIABLE
            }
            LaneKind::ReliableUnordered | LaneKind::ReliableOrdered => SendFlags::RELIABLE,
        };

        self.net.send_message(buf, send_flags)
            .map_err(SteamTransportError::Send)?;
        todo!()
    }

    pub fn update(&mut self) -> (Vec<ClientEvent<P>>, Result<(), SteamTransportError<P>>) {
        todo!()
    }
}

impl<P> SteamClientTransport<P>
where
    P: LaneProtocol,
    P::Send: TryAsBytes + OnLane<Lane = P::Lane>,
    P::Recv: TryFromBytes,
{
    pub fn connecting_to_peer(
        steam: &steamworks::Client<ClientManager>,
        remote: SteamId,
    ) -> Result<Self, SteamTransportError<P>> {
        WorkingClient::connect_to_peer(steam, remote).map(Self::Working)
    }

    pub fn connecting_to_server(
        steam: &steamworks::Client<ClientManager>,
        remote: SocketAddr,
    ) -> Result<Self, SteamTransportError<P>> {
        WorkingClient::connect_to_server(steam, remote).map(Self::Working)
    }

    pub fn connect_to_peer(
        &mut self,
        steam: &steamworks::Client<ClientManager>,
        remote: SteamId,
    ) -> Result<(), SteamTransportError<P>> {
        match self {
            Self::Disconnected => {
                *self = Self::connecting_to_peer(steam, remote)?;
                Ok(())
            }
            Self::Working(_) => Err(SteamTransportError::AlreadyConnected),
        }
    }

    pub fn connect_to_server(
        &mut self,
        steam: &steamworks::Client<ClientManager>,
        remote: SocketAddr,
    ) -> Result<(), SteamTransportError<P>> {
        match self {
            Self::Disconnected => {
                *self = Self::connecting_to_server(steam, remote)?;
                Ok(())
            }
            Self::Working(_) => Err(SteamTransportError::AlreadyConnected),
        }
    }

    pub fn disconnect(&mut self) -> Result<(), SteamTransportError<P>> {
        match self {
            Self::Disconnected => Err(SteamTransportError::AlreadyDisconnected),
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
    P::Send: TryAsBytes + OnLane<Lane = P::Lane>,
    P::Recv: TryFromBytes,
{
    type Error = SteamTransportError<P>;

    type ClientInfo = ClientInfo;

    fn client_state(&self) -> ClientState<Self::ClientInfo> {
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

    fn send(&self, msg: impl Into<P::Send>) -> Result<MessageTicket, Self::Error> {
        match self {
            Self::Disconnected => Err(SteamTransportError::NotConnected),
            Self::Working(client) => client.send(msg),
        }
    }

    fn update(&mut self) -> impl Iterator<Item = ClientEvent<P>> {
        match self {
            Self::Disconnected => vec![],
            Self::Working(client) => match client.update() {
                (events, Ok(())) => events,
                (mut events, Err(reason)) => {
                    events.push(ClientEvent::Disconnected { reason });
                    events
                }
            },
        }
        .into_iter()
    }
}
