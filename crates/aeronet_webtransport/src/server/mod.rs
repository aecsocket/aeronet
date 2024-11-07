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
        IoSet, Session, SessionEndpoint,
        connection::{DisconnectReason, Disconnected, LocalAddr, PeerAddr},
        packet::{PacketRtt, RecvPacket},
        server::{CloseReason, Closed, Opened, Server},
    },
    bevy_app::prelude::*,
    bevy_ecs::{prelude::*, system::EntityCommand},
    bevy_hierarchy::BuildChildren,
    bevy_reflect::prelude::*,
    bytes::Bytes,
    core::{net::SocketAddr, time::Duration},
    derive_more::{Display, Error, From},
    futures::channel::{mpsc, oneshot},
    tracing::{Instrument, debug_span},
    web_time::Instant,
    wtransport::error::ConnectionError,
};

/// Allows using [`WebTransportServer`].
#[derive(Debug)]
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
                (poll_servers, poll_clients)
                    .in_set(IoSet::Poll)
                    .before(session::poll),
            )
            .observe(on_server_added)
            .observe(on_connection_response);
    }
}

/// WebTransport server implementation which listens for client connections,
/// and coordinates messaging between multiple clients.
///
/// Use [`WebTransportServer::open`] to start opening a server.
#[derive(Debug, Component)]
pub struct WebTransportServer(Frontend);

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
    ///     .with_identity(&identity)
    ///     .build();
    ///
    /// // using `Commands`
    /// commands.spawn_empty().add(WebTransportServer::open(config));
    ///
    /// // using mutable `World` access
    /// # let config: ServerConfig = unimplemented!();
    /// let server = world.spawn_empty().id();
    /// WebTransportServer::open(config).apply(server, world);
    /// # }
    /// ```
    #[must_use]
    pub fn open(config: impl Into<ServerConfig>) -> impl EntityCommand {
        let config = config.into();
        |server: Entity, world: &mut World| open(server, world, config)
    }
}

fn open(server: Entity, world: &mut World, config: ServerConfig) {
    let runtime = world.resource::<WebTransportRuntime>().clone();
    let (send_closed, recv_closed) = oneshot::channel::<CloseReason<ServerError>>();
    let (send_next, recv_next) = oneshot::channel::<ToOpen>();
    runtime.spawn_on_self(
        async move {
            let Err(err) = backend::start(config, send_next).await;
            _ = send_closed.send(CloseReason::Error(err));
        }
        .instrument(debug_span!("server", %server)),
    );

    world
        .entity_mut(server)
        .insert(WebTransportServer(Frontend::Opening {
            recv_closed,
            recv_next,
        }));
}

/// How should a [`WebTransportServer`] respond to a client wishing to connect
/// to the server?
///
/// After observing a [`Trigger<SessionRequest>`], trigger this event on the
/// client to determine if the client should be allowed to connect or not.
///
/// If you do not trigger [`SessionResponse`], then the client will never
/// connect.
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
/// fn on_session_request(trigger: Trigger<SessionRequest>, mut commands: Commands) {
///     let client = trigger.entity();
///     commands.trigger_targets(SessionResponse::Accepted, client);
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
/// fn on_session_request(trigger: Trigger<SessionRequest>, mut commands: Commands) {
///     let client = trigger.entity();
///     let request = trigger.event();
///
///     let mut response = SessionResponse::Forbidden;
///     if let Some(auth_token) = request.headers.get(":auth-token") {
///         if validate_auth_token(auth_token) {
///             response = SessionResponse::Accepted;
///         }
///     }
///
///     commands.trigger_targets(response, client);
/// }
/// # fn validate_auth_token(_: &str) -> bool { unimplemented!() }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Event, Reflect)]
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
/// connection or not by triggering [`SessionResponse`] on this client.
///
/// If you do not trigger [`SessionResponse`], then the client will never
/// connect.
#[derive(Debug, Clone, PartialEq, Eq, Event, Reflect)]
pub struct SessionRequest {
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
}

/// [`WebTransportServer`] error.
#[derive(Debug, Display, Error, From)]
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
    /// Generic session error.
    #[from]
    Session(SessionError),
}

#[derive(Debug, Component)]
enum Frontend {
    Opening {
        recv_closed: oneshot::Receiver<CloseReason<ServerError>>,
        recv_next: oneshot::Receiver<ToOpen>,
    },
    Open {
        recv_closed: oneshot::Receiver<CloseReason<ServerError>>,
        recv_connecting: mpsc::Receiver<ToConnecting>,
    },
    Closed,
}

#[derive(Debug, Component)]
enum ClientFrontend {
    Connecting {
        send_session_response: Option<oneshot::Sender<SessionResponse>>,
        recv_dc: oneshot::Receiver<DisconnectReason<ServerError>>,
        recv_next: oneshot::Receiver<ToConnected>,
    },
    Connected {
        recv_dc: oneshot::Receiver<DisconnectReason<ServerError>>,
    },
    Disconnected,
}

#[derive(Debug)]
struct ToOpen {
    local_addr: SocketAddr,
    recv_connecting: mpsc::Receiver<ToConnecting>,
}

#[derive(Debug)]
struct ToConnecting {
    authority: String,
    path: String,
    origin: Option<String>,
    user_agent: Option<String>,
    headers: std::collections::HashMap<String, String>,
    send_session_entity: oneshot::Sender<Entity>,
    send_session_response: oneshot::Sender<SessionResponse>,
    recv_dc: oneshot::Receiver<DisconnectReason<ServerError>>,
    recv_next: oneshot::Receiver<ToConnected>,
}

#[derive(Debug)]
struct ToConnected {
    initial_peer_addr: SocketAddr,
    initial_rtt: Duration,
    initial_mtu: usize,
    recv_meta: mpsc::Receiver<SessionMeta>,
    recv_packet_b2f: mpsc::UnboundedReceiver<RecvPacket>,
    send_packet_f2b: mpsc::UnboundedSender<Bytes>,
    send_user_dc: oneshot::Sender<String>,
}

// TODO: required components
fn on_server_added(trigger: Trigger<OnAdd, WebTransportServer>, mut commands: Commands) {
    let server = trigger.entity();
    commands.entity(server).insert(Server);
}

fn poll_servers(mut commands: Commands, mut servers: Query<(Entity, &mut WebTransportServer)>) {
    for (server, mut frontend) in &mut servers {
        replace_with::replace_with_or_abort(&mut frontend.0, |state| match state {
            Frontend::Opening {
                recv_closed,
                recv_next,
            } => poll_opening(&mut commands, server, recv_closed, recv_next),
            Frontend::Open {
                recv_closed,
                recv_connecting,
            } => poll_open(&mut commands, server, recv_closed, recv_connecting),
            Frontend::Closed => state,
        });
    }
}

fn poll_opening(
    commands: &mut Commands,
    server: Entity,
    mut recv_closed: oneshot::Receiver<CloseReason<ServerError>>,
    mut recv_next: oneshot::Receiver<ToOpen>,
) -> Frontend {
    if should_close(commands, server, &mut recv_closed) {
        return Frontend::Closed;
    }

    let Ok(Some(next)) = recv_next.try_recv() else {
        return Frontend::Opening {
            recv_closed,
            recv_next,
        };
    };

    commands
        .entity(server)
        .insert((Opened::now(), LocalAddr(next.local_addr)));
    Frontend::Open {
        recv_closed,
        recv_connecting: next.recv_connecting,
    }
}

fn poll_open(
    commands: &mut Commands,
    server: Entity,
    mut recv_closed: oneshot::Receiver<CloseReason<ServerError>>,
    mut recv_connecting: mpsc::Receiver<ToConnecting>,
) -> Frontend {
    if should_close(commands, server, &mut recv_closed) {
        return Frontend::Closed;
    }

    while let Ok(Some(connecting)) = recv_connecting.try_next() {
        let session = commands
            // spawn -> parent -> insert, so that Parent is available
            // as soon as other components are added
            .spawn_empty()
            .set_parent(server)
            .insert((
                SessionEndpoint, // TODO: required component of ClientFrontend
                ClientFrontend::Connecting {
                    send_session_response: Some(connecting.send_session_response),
                    recv_dc: connecting.recv_dc,
                    recv_next: connecting.recv_next,
                },
            ))
            .id();
        _ = connecting.send_session_entity.send(session);

        // TODO: there may be a way to trigger SessionRequest on &mut World,
        // immediately get a SessionResponse, and respond immediately
        // without having to store send_session_response in Connecting
        // https://github.com/bevyengine/bevy/pull/14894
        let request = SessionRequest {
            authority: connecting.authority,
            path: connecting.path,
            origin: connecting.origin,
            user_agent: connecting.user_agent,
            headers: connecting.headers,
        };
        commands.trigger_targets(request, session);
    }

    Frontend::Open {
        recv_closed,
        recv_connecting,
    }
}

fn should_close(
    commands: &mut Commands,
    server: Entity,
    recv_closed: &mut oneshot::Receiver<CloseReason<ServerError>>,
) -> bool {
    let close_reason = match recv_closed.try_recv() {
        Ok(None) => None,
        Ok(Some(close_reason)) => Some(close_reason),
        Err(_) => Some(ServerError::Session(SessionError::BackendClosed).into()),
    };
    close_reason.map_or(false, |reason| {
        let reason = reason.map_err(anyhow::Error::new);
        commands.trigger_targets(Closed { reason }, server);
        true
    })
}

fn on_connection_response(
    trigger: Trigger<SessionResponse>,
    mut clients: Query<&mut ClientFrontend>,
) {
    let client = trigger.entity();
    let Ok(mut frontend) = clients.get_mut(client) else {
        return;
    };
    let ClientFrontend::Connecting {
        send_session_response,
        ..
    } = frontend.as_mut()
    else {
        return;
    };
    let Some(sender) = send_session_response.take() else {
        return;
    };

    _ = sender.send(*trigger.event());
}

fn poll_clients(mut commands: Commands, mut clients: Query<(Entity, &mut ClientFrontend)>) {
    for (client, mut frontend) in &mut clients {
        replace_with::replace_with_or_abort(&mut *frontend, |state| match state {
            ClientFrontend::Connecting {
                send_session_response,
                recv_dc,
                recv_next,
            } => poll_connecting(
                &mut commands,
                client,
                send_session_response,
                recv_dc,
                recv_next,
            ),
            ClientFrontend::Connected { mut recv_dc } => {
                if should_disconnect(&mut commands, client, &mut recv_dc) {
                    ClientFrontend::Disconnected
                } else {
                    ClientFrontend::Connected { recv_dc }
                }
            }
            ClientFrontend::Disconnected => state,
        });
    }
}

fn poll_connecting(
    commands: &mut Commands,
    entity: Entity,
    send_session_response: Option<oneshot::Sender<SessionResponse>>,
    mut recv_dc: oneshot::Receiver<DisconnectReason<ServerError>>,
    mut recv_next: oneshot::Receiver<ToConnected>,
) -> ClientFrontend {
    if should_disconnect(commands, entity, &mut recv_dc) {
        return ClientFrontend::Disconnected;
    }

    let Ok(Some(next)) = recv_next.try_recv() else {
        return ClientFrontend::Connecting {
            send_session_response,
            recv_dc,
            recv_next,
        };
    };

    let mut session = Session::new(Instant::now(), MIN_MTU);
    if let Err(err) = session.set_mtu(next.initial_mtu) {
        commands.trigger_targets(
            Disconnected {
                reason: DisconnectReason::Error(SessionError::MtuTooSmall(err).into()),
            },
            entity,
        );
        return ClientFrontend::Disconnected;
    }

    commands.entity(entity).insert((
        WebTransportIo {
            recv_meta: next.recv_meta,
            recv_packet_b2f: next.recv_packet_b2f,
            send_packet_f2b: next.send_packet_f2b,
            send_user_dc: Some(next.send_user_dc),
        },
        session,
        PeerAddr(next.initial_peer_addr),
        PacketRtt(next.initial_rtt),
    ));
    ClientFrontend::Connected { recv_dc }
}

fn should_disconnect(
    commands: &mut Commands,
    client: Entity,
    recv_dc: &mut oneshot::Receiver<DisconnectReason<ServerError>>,
) -> bool {
    let dc_reason = match recv_dc.try_recv() {
        Ok(None) => None,
        Ok(Some(dc_reason)) => Some(dc_reason),
        Err(_) => Some(ServerError::Session(SessionError::BackendClosed).into()),
    };
    dc_reason.map_or(false, |reason| {
        let reason = reason.map_err(anyhow::Error::new);
        commands.trigger_targets(Disconnected { reason }, client);
        true
    })
}
