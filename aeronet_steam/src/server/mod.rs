mod wrapper;

pub use wrapper::*;

use std::{marker::PhantomData, net::SocketAddr};

use aeronet::{
    client::{ClientKey, ClientState},
    LaneConfig, OnLane, ProtocolVersion, TransportProtocol, TryAsBytes, TryFromBytes,
};
use ahash::AHashMap;
use derivative::Derivative;
use slotmap::SlotMap;
use steamworks::{
    networking_sockets::ListenSocket,
    networking_types::{
        ConnectedEvent, ConnectionRequest, DisconnectedEvent, ListenSocketEvent, NetConnectionEnd,
    },
    Manager, ServerManager, SteamId,
};
use tracing::warn;

use crate::{shared::ConnectionFrontend, ConnectionInfo};

type SteamTransportError<P> =
    crate::SteamTransportError<<P as TransportProtocol>::S2C, <P as TransportProtocol>::C2S>;

type ServerEvent<P> = aeronet::server::ServerEvent<P, SteamTransportError<P>>;

#[derive(Debug)]
pub struct SteamServerTransportConfig {
    pub version: ProtocolVersion,
    pub max_packet_len: usize,
    pub lanes: Vec<LaneConfig>,
    pub target: ListenTarget,
}

#[derive(Debug, Clone, Copy)]
pub enum ListenTarget {
    Ip(SocketAddr),
    Peer { virtual_port: i32 },
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct OpenServer<P, M = ServerManager> {
    #[derivative(Debug = "ignore")]
    sock: ListenSocket<M>,
    config: SteamServerTransportConfig,
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
        conn: ConnectionFrontend<M>,
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
    P: TransportProtocol,
    P::C2S: TryAsBytes + TryFromBytes + OnLane,
    P::S2C: TryAsBytes + TryFromBytes + OnLane,
    M: Manager + Send + Sync + 'static,
{
    pub fn open(
        steam: &steamworks::Client<M>,
        config: SteamServerTransportConfig,
    ) -> Result<Self, SteamTransportError<P>> {
        let socks = steam.networking_sockets();
        let sock = match config.target {
            ListenTarget::Ip(addr) => socks.create_listen_socket_ip(addr, []),
            ListenTarget::Peer { virtual_port } => socks.create_listen_socket_p2p(virtual_port, []),
        }
        .map_err(|_| SteamTransportError::<P>::CreateListenSocket)?;

        Ok(Self {
            sock,
            config,
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
            Some(Client::Connected { steam_id, conn, .. }) => {
                ClientState::Connected(RemoteConnectedInfo {
                    steam_id: *steam_id,
                    info: conn.info.clone(),
                })
            }
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

    pub fn disconnect(&mut self, client: ClientKey) -> Result<(), SteamTransportError<P>> {
        match self.clients.remove(client) {
            Some(_) => Ok(()),
            None => Err(SteamTransportError::<P>::NoClient { client }),
        }
    }

    pub fn send(
        &mut self,
        client: ClientKey,
        msg: impl Into<P::S2C>,
    ) -> Result<(), SteamTransportError<P>> {
        let Some(Client::Connected { conn, .. }) = self.clients.get_mut(client) else {
            return Err(SteamTransportError::<P>::NotConnected);
        };
        conn.send(msg.into())
    }

    pub fn poll(&mut self) -> Vec<ServerEvent<P>> {
        let mut events = Vec::new();

        while let Some(event) = self.sock.try_receive_event() {
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

        let mut clients_to_remove = Vec::new();
        for (client_key, client) in self.clients.iter_mut() {
            if let Err(reason) = Self::poll_client(client_key, client, &mut events) {
                events.push(ServerEvent::Disconnected {
                    client: client_key,
                    reason,
                });
                clients_to_remove.push(client_key);
            }
        }

        for client_key in clients_to_remove {
            self.clients.remove(client_key);
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

        *client = Client::Connected {
            steam_id,
            conn: ConnectionFrontend::new(conn, self.config.max_packet_len, &self.config.lanes),
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
    ) -> Result<(), SteamTransportError<P>> {
        match client {
            Client::Requesting { .. } => Ok(()),
            Client::Connected { conn, .. } => {
                for msg in conn.recv() {
                    let msg = msg?;
                    events.push(ServerEvent::Recv {
                        client: client_key,
                        msg,
                    });
                }
                Ok(())
            }
        }
    }
}
