mod wrapper;

pub use wrapper::*;

use std::{marker::PhantomData, net::SocketAddr, time::Duration};

use aeronet::{
    ClientKey, ClientState, LaneProtocol, OnLane, TransportProtocol, TryAsBytes, TryFromBytes,
};
use ahash::AHashMap;
use derivative::Derivative;
use slotmap::SlotMap;
use steamworks::{
    networking_sockets::{InvalidHandle, ListenSocket, NetConnection, NetworkingSockets},
    networking_types::{
        ConnectedEvent, ConnectionRequest, DisconnectedEvent, ListenSocketEvent, NetConnectionEnd,
    },
    Manager, ServerManager, SteamId,
};
use tracing::warn;

use crate::ConnectionInfo;

type SteamTransportError<P> =
    crate::SteamTransportError<<P as TransportProtocol>::S2C, <P as TransportProtocol>::C2S>;

type ServerEvent<P> = aeronet::ServerEvent<P, SteamTransportError<P>>;

const RECV_BATCH_SIZE: usize = 32;

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct OpenServer<P, M = ServerManager> {
    #[derivative(Debug = "ignore")]
    sockets: NetworkingSockets<M>,
    #[derivative(Debug = "ignore")]
    socket: ListenSocket<M>,
    clients: SlotMap<ClientKey, Client<M>>,
    id_to_client: AHashMap<SteamId, ClientKey>,
    #[derivative(Debug = "ignore")]
    _phantom: PhantomData<P>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
enum Client<M> {
    Requesting {
        steam_id: SteamId,
        #[derivative(Debug = "ignore")]
        req: Option<ConnectionRequest<M>>,
    },
    Connected {
        steam_id: SteamId,
        info: ConnectionInfo,
        #[derivative(Debug = "ignore")]
        conn: NetConnection<M>,
    },
}

#[derive(Debug, Clone)]
pub struct RemoteConnectingInfo {
    pub steam_id: SteamId,
}

#[derive(Debug, Clone)]
pub struct RemoteConnectedInfo {
    pub steam_id: SteamId,
    pub info: ConnectionInfo,
}

impl<P, M> OpenServer<P, M>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
    M: Manager + Send + Sync + 'static,
{
    pub fn open_ip(
        steam: &steamworks::Client<M>,
        addr: SocketAddr,
    ) -> Result<Self, SteamTransportError<P>> {
        Self::open(
            steam.networking_sockets(),
            steam.networking_sockets().create_listen_socket_ip(addr, []),
        )
    }

    pub fn open_p2p(
        steam: &steamworks::Client<M>,
        virtual_port: i32,
    ) -> Result<Self, SteamTransportError<P>> {
        Self::open(
            steam.networking_sockets(),
            steam
                .networking_sockets()
                .create_listen_socket_p2p(virtual_port, []),
        )
    }

    fn open(
        sockets: NetworkingSockets<M>,
        sock: Result<ListenSocket<M>, InvalidHandle>,
    ) -> Result<Self, SteamTransportError<P>> {
        let socket = sock.map_err(|_| SteamTransportError::<P>::CreateListenSocket)?;
        Ok(Self {
            sockets,
            socket,
            clients: SlotMap::default(),
            id_to_client: AHashMap::default(),
            _phantom: PhantomData,
        })
    }

    #[must_use]
    pub fn client_state(
        &self,
        client: ClientKey,
    ) -> ClientState<RemoteConnectingInfo, RemoteConnectedInfo> {
        match self.clients.get(client) {
            None => ClientState::Disconnected,
            Some(Client::Requesting { steam_id, .. }) => {
                ClientState::Connecting(RemoteConnectingInfo {
                    steam_id: *steam_id,
                })
            }
            Some(Client::Connected {
                steam_id: id, info, ..
            }) => ClientState::Connected(RemoteConnectedInfo {
                steam_id: *id,
                info: info.clone(),
            }),
        }
    }

    pub fn client_keys(&self) -> impl Iterator<Item = ClientKey> + '_ {
        self.clients.keys()
    }

    pub fn accept_request(&mut self, client: ClientKey) -> Result<(), SteamTransportError<P>> {
        self.respond_to_request(client, |req| {
            let _ = req.accept();
        })
    }

    pub fn reject_request(&mut self, client: ClientKey) -> Result<(), SteamTransportError<P>> {
        self.respond_to_request(client, |req| {
            req.reject(NetConnectionEnd::AppGeneric, None);
        })
    }

    fn respond_to_request(
        &mut self,
        client: ClientKey,
        f: impl FnOnce(ConnectionRequest<M>),
    ) -> Result<(), SteamTransportError<P>> {
        let Some(Client::Requesting { req, .. }) = self.clients.get_mut(client) else {
            return Err(SteamTransportError::<P>::NoClient { client });
        };

        match req.take() {
            Some(req) => {
                f(req);
                Ok(())
            }
            None => Err(SteamTransportError::<P>::AlreadyRespondedToRequest),
        }
    }

    pub fn send(
        &mut self,
        client: ClientKey,
        msg: impl Into<P::S2C>,
    ) -> Result<(), SteamTransportError<P>> {
        let Some(Client::Connected { info, conn, .. }) = self.clients.get(client) else {
            return Err(SteamTransportError::<P>::NotConnected { client });
        };

        conn.send_message(data, send_flags)
    }

    pub fn disconnect(&mut self, client: ClientKey) -> Result<(), SteamTransportError<P>> {
        todo!()
    }

    pub fn poll(&mut self) -> Vec<ServerEvent<P>> {
        let mut events = Vec::new();

        while let Some(event) = self.socket.try_receive_event() {
            match event {
                ListenSocketEvent::Connecting(req) => {
                    self.on_connecting(req, &mut events);
                }
                ListenSocketEvent::Connected(event) => {
                    self.on_connected(event, &mut events);
                }
                ListenSocketEvent::Disconnected(event) => {
                    self.on_disconnected(event, &mut events);
                }
            }
        }

        for (client_key, client) in self.clients.iter_mut() {
            Self::poll_client(client_key, client, &mut events);
        }

        events
    }

    fn on_connecting(&mut self, req: ConnectionRequest<M>, events: &mut Vec<ServerEvent<P>>) {
        let Some(steam_id) = req.remote().steam_id() else {
            req.reject(NetConnectionEnd::AppGeneric, None);
            return;
        };
        if self.id_to_client.contains_key(&steam_id) {
            req.reject(NetConnectionEnd::AppGeneric, None);
            return;
        }

        let client_key = self.clients.insert(Client::Requesting {
            steam_id,
            req: Some(req),
        });
        self.id_to_client.insert(steam_id, client_key);
        events.push(ServerEvent::Connecting { client: client_key });
    }

    fn on_connected(&mut self, event: ConnectedEvent<M>, events: &mut Vec<ServerEvent<P>>) {
        let Some(steam_id) = event.remote().steam_id() else {
            return;
        };
        let Some(client_key) = self.id_to_client.get(&steam_id) else {
            warn!("ID to client map did not contain {steam_id:?}");
            return;
        };
        let Some(client) = self.clients.get_mut(*client_key) else {
            warn!("Client map did not contain key {client_key}");
            return;
        };

        let conn = event.take_connection();
        // TODO setup conn - negotiate

        // if let Err(reason) = shared::configure_lanes::<P, P::S2C, P::C2S, M>(&self.socks, &conn) {
        //     events.push(ServerEvent::Disconnected {
        //         client: *client_key,
        //         reason,
        //     });
        //     return;
        // }

        // TODO!!! Use ISteamNetworkingUtils::EstimatePingTimeBetweenTwoLocations
        // to get an RTT value, and constantly update it
        let info = ConnectionInfo::new(Duration::ZERO);
        *client = Client::Connected {
            steam_id,
            info,
            conn,
        };
        events.push(ServerEvent::Connected {
            client: *client_key,
        });
    }

    fn on_disconnected(&mut self, event: DisconnectedEvent, events: &mut Vec<ServerEvent<P>>) {
        let Some(steam_id) = event.remote().steam_id() else {
            return;
        };
        let Some(client) = self.id_to_client.remove(&steam_id) else {
            return;
        };
        let Some(_) = self.clients.remove(client) else {
            return;
        };

        events.push(ServerEvent::Disconnected {
            client,
            reason: SteamTransportError::<P>::Disconnected(event.end_reason()),
        });
    }

    fn poll_client(
        client_key: ClientKey,
        client: &mut Client<M>,
        events: &mut Vec<ServerEvent<P>>,
    ) {
        match client {
            Client::Requesting { .. } => {}
            Client::Connected { info, conn, .. } => {
                let packets = conn.receive_messages(RECV_BATCH_SIZE);
                todo!()
            }
        }
    }
}
