//! See [`SteamNetServer`].

use {
    crate::{
        SteamManager, SteamworksClient,
        config::SessionConfig,
        session::{SessionError, SteamNetIo, SteamNetSessionPlugin, entity_to_user_data},
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
    steamworks::{
        SteamId,
        networking_sockets::ListenSocket,
        networking_types::{
            ConnectedEvent, ConnectionRequest, ListenSocketEvent, NetConnectionEnd,
        },
    },
    sync_wrapper::SyncWrapper,
    tracing::{debug, debug_span, warn},
};

/// Allows using [`SteamNetServer`].
pub struct SteamNetServerPlugin<M: SteamManager> {
    _phantom: PhantomData<M>,
}

impl<M: SteamManager> Default for SteamNetServerPlugin<M> {
    fn default() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }
}

impl<M: SteamManager> Plugin for SteamNetServerPlugin<M> {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<SteamNetSessionPlugin<M>>() {
            app.add_plugins(SteamNetSessionPlugin::<M>::default());
        }

        app.add_systems(
            PreUpdate,
            (poll_opening::<M>, poll_opened::<M>).in_set(IoSet::Poll),
        )
        .add_observer(on_remove_client::<M>);
    }
}

/// Steam socket server which allows clients to connect to it via either a
/// socket address or as a Steam peer.
///
/// Use [`SteamNetServer::open`] to start opening a server.
///
/// When a client attempts to connect, the server will trigger a
/// [`SessionRequest`]. Your app **must** observe this, and use
/// [`SessionRequest::respond`] to set how the server should respond to this
/// connection attempt.
#[derive(Component)]
#[require(ServerEndpoint)]
pub struct SteamNetServer<M: SteamManager> {
    _phantom: PhantomData<M>,
    mtu: usize,
    clients: HashMap<SteamId, Entity>,
}

impl<M: SteamManager> SteamNetServer<M> {
    /// Gets the [`Entity`] of a [`Session`] for a Steam user, if one exists.
    #[must_use]
    pub fn client_by_steam_id(&self, steam_id: SteamId) -> Option<Entity> {
        self.clients.get(&steam_id).copied()
    }
}

/// Marks a client connected to a [`SteamNetServer`].
#[derive(Debug, Component)]
#[require(SessionEndpoint)]
pub struct SteamNetServerClient<M: SteamManager> {
    _phantom: PhantomData<M>,
    steam_id: SteamId,
}

impl<M: SteamManager> SteamNetServerClient<M> {
    /// Gets the Steam ID of the connecting client.
    #[must_use]
    pub const fn steam_id(&self) -> SteamId {
        self.steam_id
    }
}

/// Where a [`SteamNetServer`] will listen on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ListenTarget {
    /// Listen on a socket address.
    Addr(SocketAddr),
    /// Listen on other Steam users in a peer-to-peer configuration.
    Peer {
        /// Steam-specific application port to listen on.
        ///
        /// This acts like [`SocketAddr::port`] but specialized for Steam's P2P
        /// networking.
        virtual_port: i32,
    },
}

impl From<SocketAddr> for ListenTarget {
    fn from(value: SocketAddr) -> Self {
        Self::Addr(value)
    }
}

impl<M: SteamManager> SteamNetServer<M> {
    /// Creates an [`EntityCommand`] to set up a server and have it start
    /// listening for connections.
    ///
    /// # Examples
    ///
    /// ```
    /// use {
    ///     aeronet_steam::{SessionConfig, server::SteamNetServer},
    ///     bevy_ecs::prelude::*,
    ///     std::net::SocketAddr,
    ///     steamworks::ServerManager,
    /// };
    ///
    /// # fn run(mut commands: Commands, world: &mut World) {
    /// let config = SessionConfig::default();
    /// let target = "127.0.0.1:27015".parse::<SocketAddr>().unwrap();
    ///
    /// // using `Commands`
    /// commands
    ///     .spawn_empty()
    ///     .queue(SteamNetServer::<ServerManager>::open(config, target));
    ///
    /// // using mutable `World` access
    /// # let config = unimplemented!();
    /// let server = world.spawn_empty().id();
    /// SteamNetServer::<ServerManager>::open(config, target).apply(world.entity_mut(server));
    /// # }
    /// ```
    #[must_use]
    pub fn open(config: SessionConfig, target: impl Into<ListenTarget>) -> impl EntityCommand {
        let target = target.into();
        move |entity: EntityWorldMut| open::<M>(entity, config, target)
    }
}

fn open<M: SteamManager>(mut entity: EntityWorldMut, config: SessionConfig, target: ListenTarget) {
    let mtu = config.send_buffer_size;
    let sockets = entity
        .world()
        .resource::<SteamworksClient<M>>()
        .networking_sockets();
    let (send_next, recv_next) = oneshot::channel::<OpenResult<M>>();
    blocking::unblock(move || {
        let result = match target {
            ListenTarget::Addr(addr) => sockets.create_listen_socket_ip(addr, config.to_options()),
            ListenTarget::Peer { virtual_port } => {
                sockets.create_listen_socket_p2p(virtual_port, config.to_options())
            }
        };
        _ = send_next.send(result.map_err(|_| SessionError::Steam));
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

/// How should a [`SteamNetServer`] respond to a client wishing to connect to
/// the server?
///
/// See [`SessionRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionResponse {
    /// Allow the client to connect to the server.
    Accepted,
    /// Reject the client with the given reason.
    Rejected {
        /// Reason to pass to [`Disconnected::ByUser`].
        reason: String,
    },
}

impl SessionResponse {
    /// Creates a [`SessionResponse::Rejected`] from the given reason.
    #[must_use]
    pub fn rejected(reason: impl Into<String>) -> Self {
        Self::Rejected {
            reason: reason.into(),
        }
    }
}

/// Triggered when a client requests to connect to a [`SteamNetServer`].
///
/// Use the fields in this event to decide whether to accept the client's
/// connection or not, and respond accordingly by calling
/// [`SessionRequest::respond`].
///
/// At least one of your observers must `respond` to this request, otherwise
/// the server will default to [`SessionResponse::Rejected`].
///
/// # Examples
///
/// /// Accept all clients without any extra checks:
///
/// ```
/// use {
///     aeronet_steam::server::{SessionRequest, SessionResponse},
///     bevy_ecs::prelude::*,
/// };
///
/// fn on_session_request(mut trigger: Trigger<SessionRequest>) {
///     let client = trigger.target();
///     trigger.respond(SessionResponse::Accepted);
/// }
/// ```
///
/// Check if the client is in the user's friends list before accepting them:
///
/// ```
/// use {
///     aeronet_steam::{
///         SteamworksClient,
///         server::{SessionRequest, SessionResponse},
///     },
///     bevy_ecs::prelude::*,
///     steamworks::FriendFlags,
/// };
///
/// fn on_session_request(mut request: Trigger<SessionRequest>, steam: Res<SteamworksClient>) {
///     let friend = steam.friends().get_friend(request.steam_id);
///     if !friend.has_friend(FriendFlags::IMMEDIATE) {
///         request.respond(SessionResponse::rejected("not friend of the host"));
///         return;
///     }
///
///     request.respond(SessionResponse::Accepted);
/// }
/// ```
#[derive(Clone, Event)]
pub struct SessionRequest {
    /// Steam ID of the client requesting to connect.
    pub steam_id: SteamId,
    /// How should the server respond to this request?
    pub response: Option<SessionResponse>,
}

impl SessionRequest {
    /// Sets how the server should respond to this request.
    pub fn respond(&mut self, response: SessionResponse) {
        self.response = Some(response);
    }
}

type OpenResult<M> = Result<ListenSocket<M>, SessionError>;

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
                commands.trigger_targets(Closed::by_error(SessionError::BackendClosed), entity);
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
                ListenSocketEvent::Disconnected(_) => {
                    // already handled by session logic
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
            steam_id,
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

    commands.entity(client).insert((
        SteamNetIo {
            conn,
            mtu: server.mtu,
        },
        Session::new(Instant::now(), server.mtu),
    ));
    Ok(())
}

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
