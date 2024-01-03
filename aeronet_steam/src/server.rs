use std::{iter, marker::PhantomData, net::SocketAddr};

use aeronet::{
    ClientKey, ClientState, LaneKey, LaneKind, LaneProtocol, OnLane, ServerState,
    ServerTransport, TransportProtocol, TryAsBytes, TryFromBytes,
};
use ahash::AHashMap;
use derivative::Derivative;
use either::Either;
use slotmap::SlotMap;
use steamworks::{
    networking_sockets::{ListenSocket, NetConnection, NetworkingSockets, InvalidHandle},
    networking_types::{ListenSocketEvent, NetConnectionEnd, SendFlags},
    ServerManager, SteamId,
};

use crate::{shared, ConnectionInfo};

type SteamTransportError<P> =
    crate::SteamTransportError<<P as TransportProtocol>::S2C, <P as TransportProtocol>::C2S>;

type ServerEvent<P> = aeronet::ServerEvent<P, SteamTransportError<P>>;

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct OpenServer<P, M = ServerManager>
where
    P: LaneProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnLane<Lane = P::Lane>,
{
    #[derivative(Debug = "ignore")]
    socks: NetworkingSockets<M>,
    #[derivative(Debug = "ignore")]
    sock: ListenSocket<M>,
    clients: SlotMap<ClientKey, Client<M>>,
    steam_id_to_client: AHashMap<SteamId, ClientKey>,
    _phantom_p: PhantomData<P>,
}

#[derive(Derivative)]
#[derivative(Debug)]
enum Client<M> {
    Connecting {
        steam_id: SteamId,
    },
    Connected {
        steam_id: SteamId,
        #[derivative(Debug = "ignore")]
        conn: NetConnection<M>,
    },
}

impl<P, M> OpenServer<P, M>
where
    P: LaneProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnLane<Lane = P::Lane>,
    M: 'static,
{
    pub fn open_ip(
        steam: &steamworks::Client<M>,
        addr: SocketAddr,
    ) -> Result<Self, SteamTransportError<P>> {
        let socks = steam.networking_sockets();
        Self::open(socks, socks.create_listen_socket_ip(addr, []))
    }

    pub fn open_p2p(
        steam: &steamworks::Client<M>,
        port: i32,
    ) -> Result<Self, SteamTransportError<P>> {
        let socks = steam.networking_sockets();
        Self::open(socks, socks.create_listen_socket_p2p(port, []))
    }

    fn open(
        socks: NetworkingSockets<M>,
        sock: Result<ListenSocket<M>, InvalidHandle>,
    ) -> Result<Self, SteamTransportError<P>> {
        Ok(Self {
            socks,
            sock,
            clients: SlotMap::default(),
            steam_id_to_client: AHashMap::default(),
            _phantom_p: PhantomData::default(),
        })
    }

    pub fn info(&self) -> () {
        ()
    }

    pub fn client_state(&self, client: ClientKey) -> ClientState<ConnectionInfo> {
        match self.clients.get(client) {
            Some(Client::Connecting { .. }) => ClientState::Connecting,
            Some(Client::Connected { steam_id, conn })
        }
        todo!()
    }

    pub fn clients(&self) -> impl Iterator<Item = ClientKey> + '_ {
        self.clients.keys()
    }

    pub fn send(
        &mut self,
        client: ClientKey,
        msg: impl Into<P::S2C>,
    ) -> Result<(), SteamTransportError<P>> {
        let Some(Client::Connected { conn, .. }) = self.clients.get(client) else {
            return Err(SteamTransportError::<P>::NoClient);
        };

        let msg = msg.into();
        let bytes = msg
            .try_as_bytes()
            .map_err(SteamTransportError::<P>::Serialize)?;
        let bytes = bytes.as_ref();
        let send_flags = match msg.lane().kind() {
            LaneKind::UnreliableUnsequenced | LaneKind::UnreliableSequenced => {
                SendFlags::UNRELIABLE
            }
            LaneKind::ReliableUnordered | LaneKind::ReliableOrdered => SendFlags::RELIABLE,
        };
        conn.send_message(bytes, send_flags)
            .map_err(SteamTransportError::<P>::Send)?;
        Ok(())
    }

    pub fn disconnect(&mut self, client: ClientKey) -> Result<(), SteamTransportError<P>> {
        let Some(client) = self.clients.remove(client) else {
            return Err(SteamTransportError::<P>::NoClient);
        };

        debug_assert!(self.steam_id_to_client.contains_key(&client.steam_id()));
        self.steam_id_to_client.remove(&client.steam_id());

        if let Client::Connected { conn, .. } = client {
            conn.close(NetConnectionEnd::AppGeneric, None, false);
        }
        Ok(())
    }

    pub fn update(&mut self) -> Vec<ServerEvent<P, SteamTransportError<P>>> {
        let mut events = Vec::new();

        while let Some(event) = self.sock.try_receive_event() {
            match event {
                ListenSocketEvent::Connecting(event) => {
                    let Some(steam_id) = event.remote().steam_id() else {
                        continue;
                    };
                    let client = self.clients.insert(Client::Connecting { steam_id });
                    self.steam_id_to_client.insert(steam_id, client);

                    events.push(ServerEvent::Connecting { client });
                }
                ListenSocketEvent::Connected(event) => {
                    let Some(steam_id) = event.remote().steam_id() else {
                        continue;
                    };
                    let conn = event.take_connection();
                    if let Err(reason) =
                        shared::configure_lanes::<P, P::S2C, P::C2S>(&self.socks, &conn)
                    {
                        conn.close(NetConnectionEnd::AppGeneric, None, false);
                        events.push(ServerEvent::Disconnected { client, reason: () });
                        continue;
                    }

                    let client = self.clients.insert(Client::Connected {
                        steam_id,
                        conn: event.take_connection(),
                    });
                    self.steam_id_to_client.insert(steam_id, client);

                    events.push(ServerEvent::Connected { client });
                }
                ListenSocketEvent::Disconnected(event) => {
                    let Some(steam_id) = event.remote().steam_id() else {
                        continue;
                    };
                    let Some(client) = self.steam_id_to_client.remove(&steam_id) else {
                        continue;
                    };
                    let Some(_) = self.clients.remove(client) else {
                        continue;
                    };

                    events.push(ServerEvent::Disconnected {
                        client,
                        reason: SteamTransportError::<P>::Disconnected(event.end_reason()),
                    });
                }
            }
        }

        events
    }
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub enum SteamServerTransport<P>
where
    P: LaneProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnLane<Lane = P::Lane>,
{
    #[derivative(Default)]
    Closed,
    Open(OpenServer<P>),
}

impl<P> From<OpenServer<P>> for SteamServerTransport<P>
where
    P: LaneProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnLane<Lane = P::Lane>,
{
    fn from(value: OpenServer<P>) -> Self {
        Self::Open(value)
    }
}

impl<P> SteamServerTransport<P>
where
    P: LaneProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnLane<Lane = P::Lane>,
{
    pub fn open_new_ip(
        steam: &steamworks::Client,
        addr: SocketAddr,
    ) -> Result<Self, SteamTransportError<P>> {
        OpenServer::open_ip(steam, addr).map(Self::Open)
    }

    pub fn open_new_p2p(
        steam: &steamworks::Client,
        port: i32,
    ) -> Result<Self, SteamTransportError<P>> {
        OpenServer::open_p2p(steam, port).map(Self::Open)
    }

    pub fn open_ip(
        &mut self,
        steam: &steamworks::Client,
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
        steam: &steamworks::Client,
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
}

impl<P> ServerTransport<P> for SteamServerTransport<P>
where
    P: LaneProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnLane<Lane = P::Lane>,
{
    type Error = SteamTransportError<P>;

    type OpeningInfo = ();

    type ClientInfo = ConnectionInfo;

    fn state(&self) -> ServerState<Self::OpeningInfo> {
        match self {
            Self::Closed => ServerState::Closed,
            Self::Open(server) => ServerState::Open {
                info: server.info(),
            },
        }
    }

    fn client_state(&self, client: ClientKey) -> ClientState<Self::ClientInfo> {
        match self {
            Self::Closed => ClientState::Disconnected,
            Self::Open(server) => server.client_state(client),
        }
    }

    fn clients(&self) -> impl Iterator<Item = ClientKey> + '_ {
        match self {
            Self::Closed => Either::Left(iter::empty()),
            Self::Open(server) => Either::Right(server.clients()),
        }
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

    fn update(&mut self) -> impl Iterator<Item = ServerEvent<P, SteamTransportError<P>>> {
        match self {
            Self::Closed => Either::Left(iter::empty()),
            Self::Open(server) => Either::Right(server.update().into_iter()),
        }
    }
}
