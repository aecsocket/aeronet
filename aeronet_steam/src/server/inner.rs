use std::{marker::PhantomData, net::SocketAddr, time::Instant};

use aeronet::{
    ClientKey, LaneKey, LaneKind, LaneProtocol, OnLane,
    TryAsBytes, TryFromBytes,
};
use ahash::AHashMap;
use derivative::Derivative;
use slotmap::SlotMap;
use steamworks::{
    networking_sockets::{InvalidHandle, ListenSocket, NetConnection, NetworkingSockets},
    networking_types::{ConnectionRequest, ListenSocketEvent, NetConnectionEnd, SendFlags, ConnectedEvent, DisconnectedEvent},
    Manager, ServerManager, SteamId,
};
use tracing::warn;

use crate::{shared, ConnectionInfo};

use super::{SteamTransportError, ClientState, ServerEvent};

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct OpenServer<P, M = ServerManager>
where
    P: LaneProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnLane<Lane = P::Lane>,
    M: Manager + Send + Sync + 'static,
{
    #[derivative(Debug = "ignore")]
    socks: NetworkingSockets<M>,
    #[derivative(Debug = "ignore")]
    sock: ListenSocket<M>,
    clients: SlotMap<ClientKey, Client<M>>,
    steam_id_to_client: AHashMap<SteamId, ClientKey>,
    _phantom_p: PhantomData<P>,
}

#[derive(Debug, Clone)]
pub struct RemoteConnectingInfo {
    pub steam_id: SteamId,
}

#[derive(Debug, Clone)]
pub struct RemoteConnectedInfo {
    pub steam_id: SteamId,
    pub conn: ConnectionInfo,
}

#[derive(Derivative)]
#[derivative(Debug)]
enum Client<M> {
    Connecting {
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

impl<P, M> OpenServer<P, M>
where
    P: LaneProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnLane<Lane = P::Lane>,
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
        port: i32,
    ) -> Result<Self, SteamTransportError<P>> {
        Self::open(
            steam.networking_sockets(),
            steam
                .networking_sockets()
                .create_listen_socket_p2p(port, []),
        )
    }

    fn open(
        socks: NetworkingSockets<M>,
        sock: Result<ListenSocket<M>, InvalidHandle>,
    ) -> Result<Self, SteamTransportError<P>> {
        shared::assert_valid_protocol::<P>();

        let sock = sock.map_err(|_| SteamTransportError::<P>::CreateListenSocket)?;
        Ok(Self {
            socks,
            sock,
            clients: SlotMap::default(),
            steam_id_to_client: AHashMap::default(),
            _phantom_p: PhantomData::default(),
        })
    }

    pub fn client_state(&self, client: ClientKey) -> ClientState {
        match self.clients.get(client) {
            Some(Client::Connecting { steam_id, .. }) => {
                ClientState::Connecting(RemoteConnectingInfo {
                    steam_id: *steam_id,
                })
            }
            Some(Client::Connected { steam_id, info, .. }) => {
                ClientState::Connected(RemoteConnectedInfo {
                    steam_id: *steam_id,
                    conn: info.clone(),
                })
            }
            None => ClientState::Disconnected,
        }
    }

    pub fn clients(&self) -> impl Iterator<Item = ClientKey> + '_ {
        self.clients.keys()
    }

    pub fn accept_client(&mut self, client: ClientKey) -> Result<(), SteamTransportError<P>> {
        match self.clients.get_mut(client) {
            Some(Client::Connecting { req, .. }) => match req.take() {
                Some(req) => req
                    .accept()
                    .map_err(SteamTransportError::<P>::DecideSession),
                None => Err(SteamTransportError::<P>::SessionAlreadyDecided),
            },
            _ => Err(SteamTransportError::<P>::NotConnecting),
        }
    }

    pub fn reject_client(&mut self, client: ClientKey) -> Result<(), SteamTransportError<P>> {
        match self.clients.get_mut(client) {
            Some(Client::Connecting { req, .. }) => match req.take() {
                Some(req) => {
                    req.reject(NetConnectionEnd::AppGeneric, None);
                    Ok(())
                }
                None => Err(SteamTransportError::<P>::SessionAlreadyDecided),
            },
            _ => Err(SteamTransportError::<P>::NotConnecting),
        }
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

        let steam_id = match client {
            Client::Connecting { steam_id, .. } => steam_id,
            Client::Connected { steam_id, .. } => steam_id,
        };

        debug_assert!(self.steam_id_to_client.contains_key(&steam_id));
        self.steam_id_to_client.remove(&steam_id);
        Ok(())
    }

    pub fn update(&mut self) -> Vec<ServerEvent<P>> {
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

        for (client_key, client) in self.clients.iter_mut() {
            //self.handle_client::<P, M>(client_key, client, events);
        }

        events
    }

    fn on_connecting(&mut self, req: ConnectionRequest<M>, events: &mut Vec<ServerEvent<P>>) {
        let Some(steam_id) = req.remote().steam_id() else {
            req.reject(NetConnectionEnd::AppGeneric, None);
            return;
        };
        if self.steam_id_to_client.contains_key(&steam_id) {
            req.reject(NetConnectionEnd::AppGeneric, None);
            return;
        }

        let client_key = self.clients.insert(Client::Connecting {
            steam_id,
            req: Some(req),
        });
        self.steam_id_to_client.insert(steam_id, client_key);

        events.push(ServerEvent::Connecting {
            client: client_key,
            info: RemoteConnectingInfo { steam_id },
        });
    }

    fn on_connected(&mut self, event: ConnectedEvent<M>, events: &mut Vec<ServerEvent<P>>) {
        let Some(steam_id) = event.remote().steam_id() else {
            event
                .take_connection()
                .close(NetConnectionEnd::AppGeneric, None, false);
            return;
        };
        let Some(client_key) = self.steam_id_to_client.get(&steam_id) else {
            warn!("Steam ID {steam_id:?} connected, but is not in ID to client map");
            event
                .take_connection()
                .close(NetConnectionEnd::AppGeneric, None, false);
            return;
        };
        let Some(client) = self.clients.get_mut(*client_key) else {
            warn!("{client_key:?} connected, but is not in client set");
            event
                .take_connection()
                .close(NetConnectionEnd::AppGeneric, None, false);
            return;
        };

        let conn = event.take_connection();
        if let Err(reason) =
            shared::configure_lanes::<P, P::S2C, P::C2S, M>(&self.socks, &conn)
        {
            events.push(ServerEvent::Disconnected {
                client: *client_key,
                reason,
            });
            return;
        }

        let info = ConnectionInfo::default();
        *client = Client::Connected {
            steam_id,
            info: info.clone(),
            conn,
        };

        events.push(ServerEvent::Connected {
            client: *client_key,
            info: RemoteConnectedInfo {
                steam_id,
                conn: info,
            },
        });
    }

    fn on_disconnected(&mut self, event: DisconnectedEvent, events: &mut Vec<ServerEvent<P>>) {
        let Some(steam_id) = event.remote().steam_id() else {
            return;
        };
        let Some(client) = self.steam_id_to_client.remove(&steam_id) else {
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

    fn handle_client(
        client_key: ClientKey,
        client: &mut Client<M>,
        events: &mut Vec<ServerEvent<P>>,
        to_remove: &mut Vec<ClientKey>,
    )
    where
        P: LaneProtocol,
        P::C2S: TryFromBytes,
        P::S2C: TryAsBytes + OnLane<Lane = P::Lane>,
        M: Manager + Send + Sync + 'static,
    {
        let Client::Connected { info, conn, .. } = client else {
            return;
        };

        let new_events = match shared::recv_all::<P, P::S2C, P::C2S, M>(conn, info) {
            (msgs, Ok(())) => Self::map_events(client_key, msgs),
            (msgs, Err(err)) =>  {
                to_remove.push(client_key);
                Self::map_events(client_key, msgs)
            }
        };
    
        // // let events = match shared::recv_all::<P, P::S2C, P::C2S, M>(conn, info) {
        // //     (msgs, Ok(())) => Self::map_events(client_key, msgs),
        // //     (msgs, Err(err)) => 
        // // }
        // let now = Instant::now();
        // let events = msgs
        //     .into_iter()
        //     .map(|msg| ServerEvent::Recv { client: client_key, msg, at: now })
        //     .collect::<Vec<_>>();
    
        // if let Err(err) = res {
    
        // }
    }

    fn map_events(client: ClientKey, msgs: impl IntoIterator<Item = P::C2S>) -> Vec<ServerEvent<P>> {
        let msgs = msgs.into_iter();
        msgs.map(|msg| ServerEvent::Recv { client, msg, at: Instant::now() })
            .collect()
    }
}


