use std::{iter, marker::PhantomData, net::SocketAddr, time::Instant};

use aeronet::{
    ByteStats, ClientState, ClientTransport, LaneKey, LaneKind, LaneProtocol, MessageStats, OnLane,
    TransportProtocol, TryAsBytes, TryFromBytes,
};
use derivative::Derivative;
use either::Either;
use steamworks::{
    networking_sockets::{InvalidHandle, NetConnection, NetworkingSockets},
    networking_types::{NetworkingIdentity, SendFlags},
    ClientManager, SteamId,
};

use crate::shared;

type SteamTransportError<P> =
    crate::SteamTransportError<<P as TransportProtocol>::C2S, <P as TransportProtocol>::S2C>;

type ClientEvent<P> = aeronet::ClientEvent<P, SteamTransportError<P>>;

#[derive(Debug, Clone, Default)]
pub struct ClientInfo {
    pub msgs_sent: usize,
    pub msgs_recv: usize,
    pub bytes_sent: usize,
    pub bytes_recv: usize,
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
        let socks = steam.networking_sockets();
        let remote = NetworkingIdentity::new_steam_id(remote);
        Self::connect(&socks, socks.connect_p2p(remote, port, []))
    }

    pub fn connect_ip(
        steam: &steamworks::Client<M>,
        remote: SocketAddr,
    ) -> Result<Self, SteamTransportError<P>> {
        let socks = steam.networking_sockets();
        Self::connect(&socks, socks.connect_by_ip_address(remote, []))
    }

    fn connect(
        socks: &NetworkingSockets<M>,
        conn: Result<NetConnection<M>, InvalidHandle>,
    ) -> Result<Self, SteamTransportError<P>> {
        shared::assert_valid_protocol::<P>();

        let conn = conn.map_err(SteamTransportError::<P>::Connect)?;
        shared::configure_lanes::<P, P::C2S, P::S2C, M>(&socks, &conn)?;

        Ok(Self {
            conn,
            info: ClientInfo::default(),
            _phantom_p: PhantomData::default(),
        })
    }

    pub fn state(&self) -> ClientState<ClientInfo> {
        ClientState::Connected {
            info: self.info.clone(),
        }
    }

    pub fn send(&mut self, msg: impl Into<P::C2S>) -> Result<(), SteamTransportError<P>> {
        let msg = msg.into();
        let lane = msg.lane();

        let bytes = msg
            .try_as_bytes()
            .map_err(SteamTransportError::<P>::Serialize)?;
        let bytes = bytes.as_ref();

        let send_flags = match lane.kind() {
            LaneKind::UnreliableUnsequenced | LaneKind::UnreliableSequenced => {
                SendFlags::UNRELIABLE
            }
            LaneKind::ReliableUnordered | LaneKind::ReliableOrdered => SendFlags::RELIABLE,
        };

        self.conn
            .send_message(bytes, send_flags)
            .map_err(SteamTransportError::<P>::Send)?;

        self.info.msgs_sent += 1;
        self.info.bytes_sent += bytes.len();
        Ok(())
    }

    pub fn update(&mut self) -> Result<Vec<ClientEvent<P>>, SteamTransportError<P>> {
        let mut events = Vec::new();
        loop {
            let msgs = self
                .conn
                .receive_messages(64)
                .map_err(|_| SteamTransportError::<P>::LostConnection)?;
            if msgs.is_empty() {
                break;
            }

            for msg in msgs {
                let bytes = msg.data();
                let msg =
                    P::S2C::try_from_bytes(bytes).map_err(SteamTransportError::<P>::Deserialize)?;

                self.info.msgs_recv += 1;
                self.info.bytes_recv += bytes.len();
                events.push(ClientEvent::Recv {
                    msg,
                    at: Instant::now(),
                });
            }
        }

        Ok(events)
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

    fn update(&mut self) -> impl Iterator<Item = ClientEvent<P>> {
        match self {
            Self::Disconnected => Either::Left(iter::empty()),
            Self::Connected(client) => Either::Right(match client.update() {
                Ok(events) => Either::Left(events.into_iter()),
                Err(reason) => Either::Right(iter::once(ClientEvent::Disconnected { reason })),
            }),
        }
    }
}
