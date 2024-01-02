use std::{iter, marker::PhantomData, net::SocketAddr};

use aeronet::{
    ClientState, ClientTransport, LaneKey, LaneKind, LaneProtocol, OnLane, TransportProtocol,
    TryAsBytes, TryFromBytes, ClientEvent, MessageStats, ByteStats,
};
use derivative::Derivative;
use either::Either;
use steamworks::{
    networking_sockets::NetConnection,
    networking_types::{NetworkingIdentity, SendFlags},
    ClientManager, SteamId,
};

use crate::shared;

type SteamTransportError<P> =
    crate::SteamTransportError<<P as TransportProtocol>::C2S, <P as TransportProtocol>::S2C>;

#[derive(Debug, Clone, Default)]
pub struct ClientInfo {
    msgs_sent: usize,
    msgs_recv: usize,
    bytes_sent: usize,
    bytes_recv: usize,
}

impl MessageStats for ClientInfo {
    fn msgs_sent(&self) -> usize {
        self.msgs_sent
    }

    fn msgs_recv(&self) -> usize {
        self.msgs_recv
    }
}

impl ByteStats for ClientInfo {
    fn bytes_sent(&self) -> usize {
        self.bytes_sent
    }

    fn bytes_recv(&self) -> usize {
        self.bytes_recv
    }
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct ConnectedClient<P, M = ClientManager>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    #[derivative(Debug = "ignore")]
    conn: NetConnection<M>,
    info: ClientInfo,
    #[derivative(Debug = "ignore")]
    _phantom_p: PhantomData<P>,
}

// TODO Note on drop impl:
// There already exists a Drop impl for `NetConnection`, sending the message
// "Handle was closed" on drop. Some more customisation would be nice, and
// probably `NetConnection::close` could take `&mut self` instead of `self`.

impl<P, M> ConnectedClient<P, M>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
    M: 'static,
{
    pub fn connect_p2p(
        steam: &steamworks::Client<M>,
        remote: SteamId,
        port: i32,
    ) -> Result<Self, SteamTransportError<P>> {
        shared::assert_valid_protocol::<P>();

        let socks = steam.networking_sockets();
        let conn = socks
            .connect_p2p(NetworkingIdentity::new_steam_id(remote), port, [])
            .map_err(SteamTransportError::<P>::Connect)?;

        shared::configure_lanes::<P, P::C2S, P::S2C, M>(&socks, &conn)?;

        Ok(Self {
            conn,
            info: ClientInfo::default(),
            _phantom_p: PhantomData::default(),
        })
    }

    pub fn connect_ip(
        steam: &steamworks::Client<M>,
        remote: SocketAddr,
    ) -> Result<Self, SteamTransportError<P>> {
        let socks = steam.networking_sockets();
        let conn = socks
            .connect_by_ip_address(remote, [])
            .map_err(SteamTransportError::<P>::Connect)?;

        shared::configure_lanes::<P, P::C2S, P::S2C, M>(&socks, &conn)?;

        Ok(Self {
            conn,
            info: ClientInfo::default(),
            _phantom_p: PhantomData::default(),
        })
    }

    pub fn state(&self) -> ClientState<ClientInfo> {
        ClientState::Connected { info: self.info.clone() }
    }

    pub fn send(&mut self, msg: impl Into<P::C2S>) -> Result<(), SteamTransportError<P>> {
        let msg = msg.into();
        let lane = msg.lane();

        let buf = msg
            .try_as_bytes()
            .map_err(SteamTransportError::<P>::Serialize)?;
        let buf = buf.as_ref();

        let send_flags = match lane.kind() {
            LaneKind::UnreliableUnsequenced | LaneKind::UnreliableSequenced => {
                SendFlags::UNRELIABLE
            }
            LaneKind::ReliableUnordered | LaneKind::ReliableOrdered => SendFlags::RELIABLE,
        };

        self.conn
            .send_message(buf, send_flags)
            .map_err(SteamTransportError::<P>::Send)?;

        self.info.msgs_sent += 1;
        self.info.bytes_sent += buf.len();
        Ok(())
    }

    pub fn update(&mut self) -> (Vec<ClientEvent<P, SteamTransportError<P>>>, Result<(), SteamTransportError<P>>) {
        todo!()
    }
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub enum SteamClientTransport<P, M = ClientManager>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    #[derivative(Default)]
    Disconnected,
    Connected(ConnectedClient<P, M>),
}

impl<P, M> From<ConnectedClient<P, M>> for SteamClientTransport<P, M>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    fn from(value: ConnectedClient<P, M>) -> Self {
        Self::Connected(value)
    }
}

impl<P, M> SteamClientTransport<P, M>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
    M: 'static,
{
    pub fn connect_new_ip(
        steam: &steamworks::Client<M>,
        remote: SocketAddr,
    ) -> Result<Self, SteamTransportError<P>> {
        ConnectedClient::connect_ip(steam, remote).map(Self::Connected)
    }

    pub fn connect_new_p2p(
        steam: &steamworks::Client<M>,
        remote: SteamId,
        port: i32,
    ) -> Result<Self, SteamTransportError<P>> {
        ConnectedClient::connect_p2p(steam, remote, port).map(Self::Connected)
    }

    pub fn connect_ip(
        &mut self,
        steam: &steamworks::Client<M>,
        remote: SocketAddr,
    ) -> Result<(), SteamTransportError<P>> {
        match self {
            Self::Disconnected => {
                *self = Self::connect_new_ip(steam, remote)?;
                Ok(())
            }
            Self::Connected(_) => Err(SteamTransportError::<P>::AlreadyConnected),
        }
    }

    pub fn connect_p2p(
        &mut self,
        steam: &steamworks::Client<M>,
        remote: SteamId,
        port: i32,
    ) -> Result<(), SteamTransportError<P>> {
        match self {
            Self::Disconnected => {
                *self = Self::connect_new_p2p(steam, remote, port)?;
                Ok(())
            }
            Self::Connected(_) => Err(SteamTransportError::<P>::AlreadyConnected),
        }
    }

    pub fn disconnect(&mut self) -> Result<(), SteamTransportError<P>> {
        match self {
            Self::Disconnected => Err(SteamTransportError::<P>::AlreadyDisconnected),
            Self::Connected(_) => {
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

    type Info = ClientInfo;

    fn state(&self) -> ClientState<Self::Info> {
        match self {
            Self::Disconnected => ClientState::Disconnected,
            Self::Connected(client) => client.state(),
        }
    }

    fn send(&mut self, msg: impl Into<P::C2S>) -> Result<(), Self::Error> {
        match self {
            Self::Disconnected => Err(SteamTransportError::<P>::NotConnected),
            Self::Connected(client) => client.send(msg),
        }
    }

    fn update(&mut self) -> impl Iterator<Item = ClientEvent<P, Self::Error>> {
        match self {
            Self::Disconnected => Either::Left(iter::empty()),
            Self::Connected(client) => Either::Right(
                match client.update() {
                    (events, Ok(())) => events,
                    (mut events, Err(reason)) => {
                        *self = Self::Disconnected;
                        events.push(ClientEvent::Disconnected { reason });
                        events
                    }
                }
                .into_iter(),
            ),
        }
    }
}
