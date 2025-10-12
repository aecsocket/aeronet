//! See [`SteamNetServer`].

use {
    crate::{
        SteamworksClient,
        config::SessionConfig,
        session::{SessionError, SteamNetIo, SteamNetSessionPlugin, entity_to_user_data},
    },
    aeronet_io::{
        IoSystems, Session, SessionEndpoint,
        connection::LocalAddr,
        server::{CloseReason, Closed, Server, ServerEndpoint},
    },
    anyhow::{Context, Result, bail},
    bevy_app::prelude::*,
    bevy_ecs::prelude::*,
    bevy_platform::{
        collections::{HashMap, hash_map::Entry},
        time::Instant,
    },
    core::net::SocketAddr,
    derive_more::Debug,
    steamworks::{
        SteamId,
        networking_sockets::ListenSocket,
        networking_types::{
            AppNetConnectionEnd, ConnectedEvent, ConnectionRequest, ListenSocketEvent,
            NetConnectionEnd,
        },
    },
    sync_wrapper::SyncWrapper,
    tracing::{debug, debug_span},
};

/// Allows using [`SteamNetServer`].
///
/// This does not perform Steam initialization when the plugin is built;
/// instead, it defers initialization to when a [`SteamNetServer`] is added
/// to the world. This allows you to always add this plugin, but choose at
/// runtime whether you want to use Steam or not.
pub struct SteamNetServerPlugin;

impl Plugin for SteamNetServerPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<SteamNetSessionPlugin>() {
            app.add_plugins(SteamNetSessionPlugin);
        }

        app.add_systems(
            PreUpdate,
            (poll_opening, poll_opened).in_set(IoSystems::Poll),
        )
        .add_observer(on_remove_client);
    }
}

/// Steam socket server which allows clients to connect to it via either a
/// socket address or as a Steam peer.
///
/// Use [`SteamNetServer::open`] to start opening a server.
///
/// When a client attempts to connect, the server will trigger a
/// [`SessionRequest`]. Your app **must** observe this, and eventually use
/// [`SessionRequest::respond`] to set how the server should respond to this
/// connection attempt.
#[derive(Component)]
#[require(ServerEndpoint)]
pub struct SteamNetServer {
    mtu: usize,
    clients: HashMap<SteamId, Entity>,
}

impl SteamNetServer {
    /// Gets the [`Entity`] of a [`Session`] for a Steam user, if one exists.
    #[must_use]
    pub fn client_by_steam_id(&self, steam_id: SteamId) -> Option<Entity> {
        self.clients.get(&steam_id).copied()
    }
}

/// Marks a client connected to a [`SteamNetServer`].
#[derive(Debug, Component)]
#[require(SessionEndpoint)]
pub struct SteamNetServerClient {
    steam_id: SteamId,
}

impl SteamNetServerClient {
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

impl SteamNetServer {
    /// Creates an [`EntityCommand`] to set up a server and have it start
    /// listening for connections.
    ///
    /// [`SteamworksClient`] must be present in the world before this command is
    /// applied.
    ///
    /// # Examples
    ///
    /// ```
    /// use {
    ///     aeronet_steam::{SessionConfig, server::SteamNetServer},
    ///     bevy_ecs::prelude::*,
    ///     std::net::SocketAddr,
    /// };
    ///
    /// # fn run(mut commands: Commands, world: &mut World) {
    /// let config = SessionConfig::default();
    /// let target = "127.0.0.1:27015".parse::<SocketAddr>().unwrap();
    ///
    /// // using `Commands`
    /// commands
    ///     .spawn_empty()
    ///     .queue(SteamNetServer::open(config, target));
    ///
    /// // using mutable `World` access
    /// # let config = unimplemented!();
    /// let server = world.spawn_empty().id();
    /// SteamNetServer::open(config, target).apply(world.entity_mut(server));
    /// # }
    /// ```
    #[must_use]
    pub fn open(config: SessionConfig, target: impl Into<ListenTarget>) -> impl EntityCommand {
        let target = target.into();
        move |entity: EntityWorldMut| open(entity, config, target)
    }
}

fn open(mut entity: EntityWorldMut, config: SessionConfig, target: ListenTarget) {
    let mtu = config.send_buffer_size;
    let sockets = entity
        .world()
        .resource::<SteamworksClient>()
        .networking_sockets();
    let (tx_next, rx_next) = oneshot::channel::<OpenResult>();
    blocking::unblock(move || {
        let result = match target {
            ListenTarget::Addr(addr) => sockets.create_listen_socket_ip(addr, config.to_options()),
            ListenTarget::Peer { virtual_port } => {
                sockets.create_listen_socket_p2p(virtual_port, config.to_options())
            }
        };
        _ = tx_next.send(result.map_err(|_| SessionError::Steam));
    })
    .detach();

    let entity = entity.insert((
        SteamNetServer {
            mtu,
            clients: HashMap::default(),
        },
        Opening {
            rx_next: SyncWrapper::new(rx_next),
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
        /// Reason to pass to [`DisconnectReason::ByUser`].
        ///
        /// [`DisconnectReason::ByUser`]: aeronet_io::connection::DisconnectReason::ByUser
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
/// this request will panic when dropped.
///
/// You can choose to keep this around for multiple frames until you are ready
/// to send a response, if you need to for example query an external server.
///
/// # Examples
///
/// Accept all clients without any extra checks:
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
#[derive(Debug, EntityEvent)]
pub struct SessionRequest {
    /// [`Session`] client entity requesting to connect.
    pub entity: Entity,
    /// Steam ID of the client requesting to connect.
    pub steam_id: SteamId,
    #[debug(skip)]
    request: Option<ConnectionRequest>,
}

impl SessionRequest {
    /// Determines how the server should respond to this request.
    ///
    /// # Panics
    ///
    /// Panics if called more than once.
    pub fn respond(&mut self, response: SessionResponse) {
        let request = self
            .request
            .take()
            .expect("already responded to this request");
        match response {
            SessionResponse::Accepted => {
                _ = request.accept();
            }
            SessionResponse::Rejected { reason } => {
                request.reject(
                    NetConnectionEnd::App(AppNetConnectionEnd::generic_normal()),
                    Some(&reason),
                );
            }
        }
    }
}

impl Drop for SessionRequest {
    fn drop(&mut self) {
        #[rustfmt::skip]
        assert!(
            self.request.is_none(),
            "dropped a `SessionRequest` without sending a response; you must respond to this \
             request using `SessionRequest::respond`\n
             \n
             request info: {self:#?}"
        );
    }
}

type OpenResult = Result<ListenSocket, SessionError>;

#[derive(Component)]
struct Opening {
    rx_next: SyncWrapper<oneshot::Receiver<OpenResult>>,
}

#[derive(Component)]
struct Opened {
    socket: SyncWrapper<ListenSocket>,
}

fn poll_opening(
    mut commands: Commands,
    mut servers: Query<(Entity, &mut Opening), With<SteamNetServer>>,
) {
    for (server, mut server_io) in &mut servers {
        let socket = match server_io.rx_next.get_mut().try_recv() {
            Ok(Ok(socket)) => socket,
            Ok(Err(err)) => {
                commands.trigger(Closed {
                    entity: server,
                    reason: CloseReason::by_error(err),
                });
                continue;
            }
            Err(oneshot::TryRecvError::Empty) => continue,
            Err(oneshot::TryRecvError::Disconnected) => {
                commands.trigger(Closed {
                    entity: server,
                    reason: CloseReason::by_error(SessionError::BackendClosed),
                });
                continue;
            }
        };

        commands.entity(server).remove::<Opening>().insert((
            Opened {
                socket: SyncWrapper::new(socket),
            },
            Server::new(Instant::now()),
        ));
    }
}

fn poll_opened(
    mut commands: Commands,
    mut servers: Query<(Entity, &mut Opened, &mut SteamNetServer)>,
) {
    for (server, mut server_io, mut server_state) in &mut servers {
        let span = debug_span!("poll_opened", %server);
        let _span = span.enter();

        while let Some(event) = server_io.socket.get_mut().try_receive_event() {
            match event {
                ListenSocketEvent::Connecting(request) => {
                    let remote = request.remote();
                    match on_connecting(server, &mut commands, &mut server_state, request) {
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

fn on_connecting(
    server: Entity,
    commands: &mut Commands,
    server_io: &mut SteamNetServer,
    request: ConnectionRequest,
) -> Result<()> {
    let steam_id = request
        .remote()
        .steam_id()
        .context("remote has no steam ID")?;
    let entry = match server_io.clients.entry(steam_id) {
        Entry::Occupied(entry) => {
            let client = entry.get();
            bail!("steam ID {steam_id:?} is already mapped to client {client}");
        }
        Entry::Vacant(entry) => entry,
    };

    let client = commands
        .spawn((ChildOf(server), SteamNetServerClient { steam_id }))
        .id();
    entry.insert(client);

    commands.trigger(SessionRequest {
        entity: client,
        steam_id,
        request: Some(request),
    });

    Ok(())
}

fn on_connected(
    commands: &mut Commands,
    server_io: &SteamNetServer,
    event: ConnectedEvent,
) -> Result<()> {
    let steam_id = event
        .remote()
        .steam_id()
        .context("remote has no steam ID")?;
    let client = *server_io
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
            mtu: server_io.mtu,
        },
        Session::new(Instant::now(), server_io.mtu),
    ));
    Ok(())
}

fn on_remove_client(
    trigger: On<Remove, SteamNetServerClient>,
    clients: Query<(&SteamNetServerClient, &ChildOf)>,
    mut servers: Query<&mut SteamNetServer>,
) -> Result<(), BevyError> {
    let entity = trigger.event_target();
    let (client_io, &ChildOf(server)) = clients
        .get(entity)
        .with_context(|| format!("client {entity} does not have correct components"))?;
    let mut server_io = servers.get_mut(server).with_context(|| {
        format!("client {entity} is a child of an entity, but that entity is not a server")
    })?;
    server_io.clients.remove(&client_io.steam_id);
    Ok(())
}
