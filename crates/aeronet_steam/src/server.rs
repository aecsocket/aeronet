use {
    crate::{
        SteamManager, Steamworks,
        config::SteamSessionConfig,
        session::{SteamNetIo, SteamNetSessionPlugin, entity_to_user_data},
    },
    aeronet_io::{
        IoSet, Session, SessionEndpoint,
        connection::{Disconnected, LocalAddr, UNKNOWN_DISCONNECT_REASON},
        server::{Closed, Server, ServerEndpoint},
    },
    anyhow::{Context, Result, bail},
    bevy_app::prelude::*,
    bevy_ecs::prelude::*,
    bevy_platform_support::{
        collections::{HashMap, hash_map::Entry},
        time::Instant,
    },
    core::{any::type_name, marker::PhantomData, net::SocketAddr},
    derive_more::{Display, Error},
    steamworks::{
        ClientManager, SteamId,
        networking_sockets::ListenSocket,
        networking_types::{
            ConnectedEvent, ConnectionRequest, DisconnectedEvent, ListenSocketEvent,
            NetConnectionEnd, NetworkingIdentity,
        },
    },
    sync_wrapper::SyncWrapper,
    tracing::{debug, debug_span, warn},
};

pub struct SteamNetServerPlugin;

impl Plugin for SteamNetServerPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<SteamNetSessionPlugin<ClientManager>>() {
            app.add_plugins(SteamNetSessionPlugin::<ClientManager>::default());
        }

        app.add_systems(
            PreUpdate,
            (poll_opening::<ClientManager>, poll_opened::<ClientManager>).in_set(IoSet::Poll),
        )
        .add_observer(on_remove_client::<ClientManager>);
    }
}

#[derive(Component)]
#[require(ServerEndpoint)]
pub struct SteamNetServer<M: SteamManager = ClientManager> {
    _phantom: PhantomData<M>,
    mtu: usize,
    clients: HashMap<SteamId, Entity>,
}

impl<M: SteamManager> SteamNetServer<M> {
    #[must_use]
    pub fn client_by_steam_id(&self, steam_id: SteamId) -> Option<Entity> {
        self.clients.get(&steam_id).copied()
    }
}

#[derive(Debug, Component)]
#[require(SessionEndpoint)]
pub struct SteamNetServerClient<M: SteamManager = ClientManager> {
    _phantom: PhantomData<M>,
    steam_id: SteamId,
}

impl<M: SteamManager> SteamNetServerClient<M> {
    #[must_use]
    pub const fn steam_id(&self) -> SteamId {
        self.steam_id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ListenTarget {
    Addr(SocketAddr),
    Peer { virtual_port: i32 },
}

impl From<SocketAddr> for ListenTarget {
    fn from(value: SocketAddr) -> Self {
        Self::Addr(value)
    }
}

impl SteamNetServer<ClientManager> {
    #[must_use]
    pub fn open(config: SteamSessionConfig, target: impl Into<ListenTarget>) -> impl EntityCommand {
        let target = target.into();
        move |entity: EntityWorldMut| open::<ClientManager>(entity, config, target)
    }
}

fn open<M: SteamManager>(
    mut entity: EntityWorldMut,
    config: SteamSessionConfig,
    target: ListenTarget,
) {
    let mtu = config.send_buffer_size;
    let sockets = entity
        .world()
        .resource::<Steamworks<M>>()
        .networking_sockets();
    let (send_next, recv_next) = oneshot::channel::<OpenResult<M>>();
    blocking::unblock(move || {
        let result = match target {
            ListenTarget::Addr(addr) => sockets.create_listen_socket_ip(addr, config.to_options()),
            ListenTarget::Peer { virtual_port } => {
                sockets.create_listen_socket_p2p(virtual_port, config.to_options())
            }
        };
        _ = send_next.send(result.map_err(|_| ServerError::CreateListenSocket));
    })
    .detach();

    let entity = entity.insert((
        SteamNetServer::<M> {
            _phantom: PhantomData,
            mtu,
            clients: HashMap::default(),
        },
        Opening {
            recv_next: SyncWrapper::new(recv_next),
        },
    ));
    if let ListenTarget::Addr(local_addr) = target {
        entity.insert(LocalAddr(local_addr));
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionResponse {
    Accepted,
    Rejected { reason: String },
}

impl SessionResponse {
    #[must_use]
    pub fn rejected(reason: impl Into<String>) -> Self {
        Self::Rejected {
            reason: reason.into(),
        }
    }
}

#[derive(Clone, Event)]
pub struct SessionRequest {
    pub identity: NetworkingIdentity,
    pub response: Option<SessionResponse>,
}

impl SessionRequest {
    pub fn respond(&mut self, response: SessionResponse) {
        self.response = Some(response);
    }
}

#[derive(Debug, Display, Error)]
#[non_exhaustive]
pub enum ServerError {
    #[display("backend closed")]
    BackendClosed,
    #[display("failed to create listen socket")]
    CreateListenSocket,
}

type OpenResult<M> = Result<ListenSocket<M>, ServerError>;

#[derive(Component)]
struct Opening<M> {
    recv_next: SyncWrapper<oneshot::Receiver<OpenResult<M>>>,
}

#[derive(Component)]
struct Opened<M> {
    socket: ListenSocket<M>,
}

fn poll_opening<M: SteamManager>(
    mut commands: Commands,
    mut servers: Query<(Entity, &mut Opening<M>), With<SteamNetServer<M>>>,
) {
    for (entity, mut server) in &mut servers {
        let socket = match server.recv_next.get_mut().try_recv() {
            Ok(Ok(socket)) => socket,
            Ok(Err(err)) => {
                commands.trigger_targets(Closed::by_error(err), entity);
                continue;
            }
            Err(oneshot::TryRecvError::Empty) => continue,
            Err(oneshot::TryRecvError::Disconnected) => {
                commands.trigger_targets(Closed::by_error(ServerError::BackendClosed), entity);
                continue;
            }
        };

        commands
            .entity(entity)
            .remove::<Opening<M>>()
            .insert((Opened { socket }, Server::new(Instant::now())));
    }
}

fn poll_opened<M: SteamManager>(
    mut commands: Commands,
    mut servers: Query<(Entity, &Opened<M>, &mut SteamNetServer<M>)>,
) {
    for (entity, server, mut server_state) in &mut servers {
        let span = debug_span!("poll_opened", %entity);
        let _span = span.enter();

        while let Some(event) = server.socket.try_receive_event() {
            match event {
                ListenSocketEvent::Connecting(request) => {
                    let remote = request.remote();
                    match on_connecting(entity, &mut commands, &mut server_state, request) {
                        Ok(()) => {
                            debug!("Accepted client connection from {remote:?}");
                        }
                        Err(err) => {
                            debug!("Failed to accept client connection from {remote:?}: {err:?}");
                        }
                    }
                }
                ListenSocketEvent::Connected(event) => {
                    let remote = event.remote();
                    match on_connected(&mut commands, &server_state, event) {
                        Ok(()) => {
                            debug!("Marked {remote:?} as connected");
                        }
                        Err(err) => {
                            debug!("Failed to mark {remote:?} as connected: {err:?}");
                        }
                    }
                }
                ListenSocketEvent::Disconnected(event) => {
                    // TODO: I think this is already handled by session
                    // disconnect checks let remote =
                    // event.remote(); on_disconnected(&mut
                    // commands);
                }
            }
        }
    }
}

fn on_connecting<M: SteamManager>(
    entity: Entity,
    commands: &mut Commands,
    server: &mut SteamNetServer<M>,
    request: ConnectionRequest<M>,
) -> Result<()> {
    let steam_id = request
        .remote()
        .steam_id()
        .context("remote has no steam ID")?;
    let entry = match server.clients.entry(steam_id) {
        Entry::Occupied(entry) => {
            let client = entry.get();
            bail!("steam ID {steam_id:?} is already mapped to client {client}");
        }
        Entry::Vacant(entry) => entry,
    };

    let client = commands
        .spawn((
            ChildOf { parent: entity },
            SteamNetServerClient::<M> {
                _phantom: PhantomData,
                steam_id,
            },
        ))
        .id();
    entry.insert(client);

    commands.queue(move |world: &mut World| {
        let mut event = SessionRequest {
            identity: request.remote(),
            response: None,
        };
        world.trigger_targets_ref(&mut event, client);

        let response = event.response.unwrap_or_else(|| {
            warn!(
                "Client session {client} created on server {entity} but no response was given, \
                 will not allow this client to connect; you must `respond` to `{}`",
                type_name::<SessionRequest>(),
            );
            SessionResponse::rejected(UNKNOWN_DISCONNECT_REASON)
        });

        match response {
            SessionResponse::Accepted => {
                _ = request.accept();
            }
            SessionResponse::Rejected { reason } => {
                request.reject(NetConnectionEnd::AppGeneric, Some(&reason));
                world.trigger_targets(Disconnected::by_user(reason), client);
            }
        }
    });
    Ok(())
}

fn on_connected<M: SteamManager>(
    commands: &mut Commands,
    server: &SteamNetServer<M>,
    event: ConnectedEvent<M>,
) -> Result<()> {
    let steam_id = event
        .remote()
        .steam_id()
        .context("remote has no steam ID")?;
    let client = *server
        .clients
        .get(&steam_id)
        .with_context(|| format!("steam ID {steam_id:?} is not tracked in the client map"))?;

    let conn = event.take_connection();
    let user_data = entity_to_user_data(client);
    conn.set_connection_user_data(user_data)
        .context("failed to set connection user data")?;

    conn.send_message(
        b"hello world",
        steamworks::networking_types::SendFlags::RELIABLE_NO_NAGLE,
    )
    .unwrap();

    commands.entity(client).insert((
        SteamNetIo {
            conn,
            mtu: server.mtu,
        },
        Session::new(Instant::now(), server.mtu),
    ));
    Ok(())
}

// fn on_disconnected<M: SteamManager>(
//     commands: &mut Commands,
//     server: &SteamNetServer<M>,
//     event: DisconnectedEvent,
// ) -> Result<()> {
//     let steam_id = event
//         .remote()
//         .steam_id()
//         .context("remote has no steam ID")?;
//     let client = server
//         .clients
//         .get(&steam_id)
//         .with_context(|| format!("steam ID {steam_id:?} is not tracked in the
// client map"))?;

//     let disconnected = match event.end_reason() {
//         NetConnectionEnd::AppGeneric => Disconnected::by_peer("(unknown)"),
//         err => Disconnected::by_error()
//     }

//     commands.trigger_targets(Disconnected::, targets);
// }

fn on_remove_client<M: SteamManager>(
    trigger: Trigger<OnRemove, SteamNetServerClient<M>>,
    clients: Query<(&SteamNetServerClient<M>, &ChildOf)>,
    mut servers: Query<&mut SteamNetServer<M>>,
) -> Result<(), BevyError> {
    let entity = trigger.target();
    let (client, &ChildOf { parent }) = clients
        .get(entity)
        .with_context(|| format!("client {entity} does not have correct components"))?;
    let mut server = servers.get_mut(parent).with_context(|| {
        format!("client {entity} is a child of an entity, but that entity is not a server")
    })?;
    server.clients.remove(&client.steam_id);
    Ok(())
}
