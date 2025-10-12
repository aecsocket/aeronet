//! See [`WebTransportServer`].

mod backend;

use {
    crate::{
        runtime::WebTransportRuntime,
        session::{
            self, MIN_MTU, SessionError, SessionMeta, WebTransportIo, WebTransportSessionPlugin,
        },
    },
    aeronet_io::{
        IoSystems, Session, SessionEndpoint,
        connection::{DisconnectReason, Disconnected, LocalAddr, PeerAddr},
        packet::{PacketRtt, RecvPacket},
        server::{CloseReason, Closed, Server, ServerEndpoint},
    },
    bevy_app::prelude::*,
    bevy_ecs::{prelude::*, system::EntityCommand},
    bevy_platform::time::Instant,
    bevy_reflect::prelude::*,
    bytes::Bytes,
    core::{mem, net::SocketAddr, time::Duration},
    derive_more::{Debug, Display, Error},
    futures::channel::{mpsc, oneshot},
    tracing::{Instrument, debug, debug_span},
    wtransport::error::ConnectionError,
};

/// Allows using [`WebTransportServer`].
pub struct WebTransportServerPlugin;

impl Plugin for WebTransportServerPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<WebTransportSessionPlugin>() {
            app.add_plugins(WebTransportSessionPlugin);
        }

        app.register_type::<SessionRequest>()
            .register_type::<SessionResponse>()
            .add_systems(
                PreUpdate,
                (poll_opening, poll_opened, poll_connecting, poll_connected)
                    .in_set(IoSystems::Poll)
                    .before(session::poll),
            );
    }
}

/// WebTransport server implementation which listens for client connections,
/// and coordinates messaging between multiple clients.
///
/// Use [`WebTransportServer::open`] to start opening a server.
///
/// When a client attempts to connect, the server will trigger a
/// [`SessionRequest`]. Your app **must** observe this, and eventually use
/// [`SessionRequest::respond`] to set how the server should respond to this
/// connection attempt.
#[derive(Debug, Component)]
#[require(ServerEndpoint)]
pub struct WebTransportServer(());

/// Marks a client connected to a [`WebTransportServer`].
#[derive(Debug, Component)]
#[require(SessionEndpoint)]
pub struct WebTransportServerClient(());

/// Configuration for the [`WebTransportServer`].
pub type ServerConfig = wtransport::ServerConfig;

impl WebTransportServer {
    /// Creates an [`EntityCommand`] to set up a server and have it start
    /// listening for connections.
    ///
    /// # Examples
    ///
    /// ```
    /// use {
    ///     aeronet_webtransport::server::{ServerConfig, WebTransportServer},
    ///     bevy_ecs::{prelude::*, system::EntityCommand},
    /// };
    ///
    /// # fn run(mut commands: Commands, world: &mut World) {
    /// // set up a self-signed certificate to identify this server
    /// let identity = wtransport::Identity::self_signed(["localhost", "127.0.0.1", "::1"]).unwrap();
    ///
    /// let config = ServerConfig::builder()
    ///     .with_bind_default(12345) // server port
    ///     .with_identity(identity)
    ///     .build();
    ///
    /// // using `Commands`
    /// commands
    ///     .spawn_empty()
    ///     .queue(WebTransportServer::open(config));
    ///
    /// // using mutable `World` access
    /// # let config: ServerConfig = unimplemented!();
    /// let server = world.spawn_empty().id();
    /// WebTransportServer::open(config).apply(world.entity_mut(server));
    /// # }
    /// ```
    #[must_use]
    pub fn open(config: impl Into<ServerConfig>) -> impl EntityCommand {
        let config = config.into();
        move |entity: EntityWorldMut| open(entity, config)
    }
}

fn open(mut entity: EntityWorldMut, config: ServerConfig) {
    let runtime = entity.world().resource::<WebTransportRuntime>().clone();
    let (tx_close_reason, rx_close_reason) = oneshot::channel::<CloseReason>();
    let (tx_next, rx_next) = oneshot::channel::<ToOpen>();
    runtime.spawn_on_self(
        async move {
            let Err(reason) = backend::start(config, tx_next).await;
            debug!("Server closed: {reason:?}");
            _ = tx_close_reason.send(reason);
        }
        .instrument(debug_span!("server", entity = %entity.id())),
    );

    entity.insert((
        WebTransportServer(()),
        Opening {
            rx_close_reason,
            rx_next,
        },
    ));
}

/// How should a [`WebTransportServer`] respond to a client wishing to connect
/// to the server?
///
/// See [`SessionRequest`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub enum SessionResponse {
    /// Allow the client to connect to the server.
    Accepted,
    /// Reject the client with a `403 Forbidden`.
    Forbidden,
    /// Reject the client with a `404 Not Found`.
    NotFound,
}

/// Triggered when a client requests to connect to a [`WebTransportServer`].
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
///     aeronet_webtransport::server::{SessionRequest, SessionResponse},
///     bevy_ecs::prelude::*,
/// };
///
/// fn on_session_request(mut trigger: Trigger<SessionRequest>) {
///     let client = trigger.target();
///     trigger.respond(SessionResponse::Accepted);
/// }
/// ```
///
/// Check if the client has a given header before accepting them:
///
/// ```
/// use {
///     aeronet_webtransport::server::{SessionRequest, SessionResponse},
///     bevy_ecs::prelude::*,
/// };
///
/// fn on_session_request(mut request: Trigger<SessionRequest>) {
///     let mut response = SessionResponse::Forbidden;
///     if let Some(auth_token) = request.headers.get(":auth-token") {
///         if validate_auth_token(auth_token) {
///             response = SessionResponse::Accepted;
///         }
///     }
///
///     request.respond(response);
/// }
/// # fn validate_auth_token(_: &str) -> bool { unimplemented!() }
/// ```
#[derive(Debug, EntityEvent, Reflect)]
#[reflect(from_reflect = false)]
pub struct SessionRequest {
    /// [`Session`] client entity requesting to connect.
    pub entity: Entity,
    /// `:authority` header.
    pub authority: String,
    /// `:path` header.
    pub path: String,
    /// `origin` header.
    pub origin: Option<String>,
    /// `user-agent` header.
    pub user_agent: Option<String>,
    /// Full map of request headers.
    pub headers: std::collections::HashMap<String, String>,
    #[reflect(ignore)]
    #[debug(skip)]
    tx_session_response: Option<oneshot::Sender<SessionResponse>>,
}

impl SessionRequest {
    /// Determines how the server should respond to this request.
    ///
    /// # Panics
    ///
    /// Panics if called more than once.
    pub fn respond(&mut self, response: SessionResponse) {
        let send_session_response = self
            .tx_session_response
            .take()
            .expect("already responded to this request");
        _ = send_session_response.send(response);
    }
}

impl Drop for SessionRequest {
    fn drop(&mut self) {
        #[rustfmt::skip]
        assert!(
            self.tx_session_response.is_none(),
            "dropped a `SessionRequest` without sending a response; you must respond to this \
             request using `SessionRequest::respond`\n
             \n
             request info: {self:#?}"
        );
    }
}

/// [`WebTransportServer`]-specific error.
///
/// For generic WebTransport errors, see [`SessionError`].
#[derive(Debug, Display, Error)]
#[non_exhaustive]
pub enum ServerError {
    /// Failed to await an incoming session request.
    #[display("failed to await session request")]
    AwaitSessionRequest(ConnectionError),
    /// User rejected this incoming session request.
    #[display("user rejected session request")]
    Rejected,
    /// Failed to accept the incoming session request.
    #[display("failed to accept session")]
    AcceptSessionRequest(ConnectionError),
}

#[derive(Debug, Component)]
struct Opening {
    rx_close_reason: oneshot::Receiver<CloseReason>,
    rx_next: oneshot::Receiver<ToOpen>,
}

#[derive(Debug, Component)]
struct Opened {
    rx_close_reason: oneshot::Receiver<CloseReason>,
    rx_connecting: mpsc::Receiver<ToConnecting>,
}

#[derive(Debug, Component)]
struct Connecting {
    rx_dc_reason: oneshot::Receiver<DisconnectReason>,
    rx_next: oneshot::Receiver<ToConnected>,
}

#[derive(Debug, Component)]
struct Connected {
    rx_dc_reason: oneshot::Receiver<DisconnectReason>,
}

#[derive(Debug)]
struct ToOpen {
    local_addr: SocketAddr,
    rx_connecting: mpsc::Receiver<ToConnecting>,
}

#[derive(Debug)]
struct ToConnecting {
    authority: String,
    path: String,
    origin: Option<String>,
    user_agent: Option<String>,
    headers: std::collections::HashMap<String, String>,
    tx_session_entity: oneshot::Sender<Entity>,
    tx_session_response: oneshot::Sender<SessionResponse>,
    rx_dc_reason: oneshot::Receiver<DisconnectReason>,
    rx_next: oneshot::Receiver<ToConnected>,
}

#[derive(Debug)]
struct ToConnected {
    initial_peer_addr: SocketAddr,
    initial_rtt: Duration,
    initial_mtu: usize,
    rx_meta: mpsc::Receiver<SessionMeta>,
    rx_packet_b2f: mpsc::UnboundedReceiver<RecvPacket>,
    tx_packet_f2b: mpsc::UnboundedSender<Bytes>,
    tx_user_dc: oneshot::Sender<String>,
}

fn poll_opening(
    mut commands: Commands,
    mut servers: Query<(Entity, &mut Opening), With<WebTransportServer>>,
) {
    for (entity, mut server) in &mut servers {
        if try_close(&mut commands, entity, &mut server.rx_close_reason) {
            continue;
        }

        let Ok(Some(next)) = server.rx_next.try_recv() else {
            continue;
        };
        let (_, dummy) = oneshot::channel();
        let rx_close_reason = mem::replace(&mut server.rx_close_reason, dummy);
        commands.entity(entity).remove::<Opening>().insert((
            Server::new(Instant::now()),
            Opened {
                rx_close_reason,
                rx_connecting: next.rx_connecting,
            },
            LocalAddr(next.local_addr),
        ));
    }
}

fn poll_opened(
    mut commands: Commands,
    mut servers: Query<(Entity, &mut Opened), With<WebTransportServer>>,
) {
    for (entity, mut server) in &mut servers {
        if try_close(&mut commands, entity, &mut server.rx_close_reason) {
            continue;
        }

        while let Ok(Some(connecting)) = server.rx_connecting.try_next() {
            let client = commands
                .spawn((
                    ChildOf(entity),
                    WebTransportServerClient(()),
                    Connecting {
                        rx_dc_reason: connecting.rx_dc_reason,
                        rx_next: connecting.rx_next,
                    },
                ))
                .id();
            _ = connecting.tx_session_entity.send(client);

            commands.trigger(SessionRequest {
                entity: client,
                authority: connecting.authority,
                path: connecting.path,
                origin: connecting.origin,
                user_agent: connecting.user_agent,
                headers: connecting.headers,
                tx_session_response: Some(connecting.tx_session_response),
            });
        }
    }
}

fn try_close(
    commands: &mut Commands,
    entity: Entity,
    rx_close_reason: &mut oneshot::Receiver<CloseReason>,
) -> bool {
    let close_reason = match rx_close_reason.try_recv() {
        Ok(None) => None,
        Ok(Some(closed)) => Some(closed),
        Err(_) => Some(SessionError::BackendClosed.into()),
    };
    close_reason.is_some_and(|reason| {
        commands.trigger(Closed { entity, reason });
        true
    })
}

fn poll_connecting(
    mut commands: Commands,
    mut clients: Query<(Entity, &mut Connecting), With<WebTransportServerClient>>,
) {
    for (entity, mut client) in &mut clients {
        if try_disconnect(&mut commands, entity, &mut client.rx_dc_reason) {
            continue;
        }

        let Ok(Some(next)) = client.rx_next.try_recv() else {
            continue;
        };

        let mut session = Session::new(Instant::now(), MIN_MTU);
        if let Err(err) = session.set_mtu(next.initial_mtu) {
            commands.trigger(Disconnected {
                entity,
                reason: DisconnectReason::by_error(SessionError::MtuTooSmall(err)),
            });
            continue;
        }

        let (_, dummy) = oneshot::channel();
        let rx_dc = mem::replace(&mut client.rx_dc_reason, dummy);
        commands.entity(entity).remove::<Connecting>().insert((
            WebTransportIo {
                rx_meta: next.rx_meta,
                rx_packet_b2f: next.rx_packet_b2f,
                tx_packet_f2b: next.tx_packet_f2b,
                tx_user_dc: Some(next.tx_user_dc),
            },
            Connected {
                rx_dc_reason: rx_dc,
            },
            session,
            PeerAddr(next.initial_peer_addr),
            PacketRtt(next.initial_rtt),
        ));
    }
}

fn poll_connected(
    mut commands: Commands,
    mut clients: Query<(Entity, &mut Connected), With<WebTransportServerClient>>,
) {
    for (entity, mut client) in &mut clients {
        try_disconnect(&mut commands, entity, &mut client.rx_dc_reason);
    }
}

fn try_disconnect(
    commands: &mut Commands,
    entity: Entity,
    rx_dc: &mut oneshot::Receiver<DisconnectReason>,
) -> bool {
    let dc_reason = match rx_dc.try_recv() {
        Ok(None) => None,
        Ok(Some(disconnected)) => Some(disconnected),
        Err(_) => Some(SessionError::BackendClosed.into()),
    };
    dc_reason.is_some_and(|reason| {
        commands.trigger(Disconnected { entity, reason });
        true
    })
}
