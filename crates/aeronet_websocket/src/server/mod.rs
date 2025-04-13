//! See [`WebSocketServer`].

mod backend;
mod config;

pub use config::*;
use {
    crate::{
        WebSocketRuntime,
        session::{self, SessionError, SessionFrontend, WebSocketIo, WebSocketSessionPlugin},
        tungstenite,
    },
    aeronet_io::{
        IoSet, SessionEndpoint,
        connection::{Disconnected, LocalAddr, PeerAddr},
        server::{Closed, Server, ServerEndpoint},
    },
    bevy_app::prelude::*,
    bevy_ecs::prelude::*,
    bevy_platform_support::time::Instant,
    core::{mem, net::SocketAddr},
    derive_more::{Display, Error},
    futures::channel::{mpsc, oneshot},
    std::io,
    tracing::{Instrument, debug, debug_span},
};

/// Allows using [`WebSocketServer`].
pub struct WebSocketServerPlugin;

impl Plugin for WebSocketServerPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<WebSocketSessionPlugin>() {
            app.add_plugins(WebSocketSessionPlugin);
        }

        app.add_systems(
            PreUpdate,
            (poll_opening, poll_opened, poll_connecting, poll_connected)
                .in_set(IoSet::Poll)
                .before(session::poll),
        );
    }
}

/// WebSocket server implementation which listens for client connections,
/// and coordinates messaging between multiple clients.
///
/// Use [`WebSocketServer::open`] to start opening a server.
#[derive(Debug, Component)]
#[require(ServerEndpoint)]
pub struct WebSocketServer(());

/// Marks a client connected to a [`WebSocketServer`].
#[derive(Debug, Component)]
#[require(SessionEndpoint)]
pub struct WebSocketServerClient(());

impl WebSocketServer {
    /// Creates an [`EntityCommand`] to set up a server and have it start
    /// listening for connections.
    ///
    /// # Examples
    ///
    /// ```
    /// use {
    ///     aeronet_websocket::server::{Identity, ServerConfig, WebSocketServer},
    ///     bevy_ecs::prelude::*,
    /// };
    ///
    /// # fn run(mut commands: Commands, world: &mut World) {
    /// // set up a self-signed certificate to identify this server
    /// let identity = Identity::self_signed(["localhost", "127.0.0.1", "::1"]).unwrap();
    ///
    /// let config = ServerConfig::builder()
    ///     .with_bind_default(12345) // server port
    ///     .with_identity(identity);
    ///
    /// // using `Commands`
    /// commands.spawn_empty().queue(WebSocketServer::open(config));
    ///
    /// // using mutable `World` access
    /// # let config: ServerConfig = unimplemented!();
    /// let server = world.spawn_empty().id();
    /// WebSocketServer::open(config).apply(world.entity_mut(server));
    /// # }
    /// ```
    #[must_use]
    pub fn open(config: impl Into<ServerConfig>) -> impl EntityCommand {
        let config = config.into();
        move |entity: EntityWorldMut| open(entity, config)
    }
}

fn open(mut entity: EntityWorldMut, config: ServerConfig) {
    let runtime = entity.world().resource::<WebSocketRuntime>().clone();

    let (send_closed, recv_closed) = oneshot::channel::<Closed>();
    let (send_next, recv_next) = oneshot::channel::<ToOpen>();
    runtime.spawn_on_self(
        async move {
            let Err(closed) = backend::start(config, send_next).await;
            debug!("Server closed: {closed:?}");
            _ = send_closed.send(closed);
        }
        .instrument(debug_span!("server", entity = %entity.id())),
    );

    entity.insert((
        WebSocketServer(()),
        Opening {
            recv_closed,
            recv_next,
        },
    ));
}

/// [`WebSocketServer`]-specific error.
///
/// For generic WebSocket errors, see [`SessionError`].
#[derive(Debug, Display, Error)]
#[non_exhaustive]
pub enum ServerError {
    /// Failed to bind a socket to the address given in [`ServerConfig`].
    #[display("failed to bind socket")]
    BindSocket(io::Error),
    /// Failed to accept a connection.
    #[display("failed to accept connection")]
    AcceptConnection(io::Error),
    /// Failed to perform a TLS handshake over this connection.
    #[display("failed to perform TLS handshake")]
    TlsHandshake(io::Error),
    /// Failed to accept the client due to a WebSocket protocol error.
    #[display("failed to accept client")]
    AcceptClient(tungstenite::Error),
}

#[derive(Debug, Component)]
struct Opening {
    recv_closed: oneshot::Receiver<Closed>,
    recv_next: oneshot::Receiver<ToOpen>,
}

#[derive(Debug, Component)]
struct Opened {
    recv_closed: oneshot::Receiver<Closed>,
    recv_connecting: mpsc::Receiver<ToConnecting>,
}

#[derive(Debug, Component)]
struct Connecting {
    recv_dc: oneshot::Receiver<Disconnected>,
    recv_next: oneshot::Receiver<ToConnected>,
}

#[derive(Debug, Component)]
struct Connected {
    recv_dc: oneshot::Receiver<Disconnected>,
}

#[derive(Debug)]
struct ToOpen {
    local_addr: SocketAddr,
    recv_connecting: mpsc::Receiver<ToConnecting>,
}

#[derive(Debug)]
struct ToConnecting {
    peer_addr: SocketAddr,
    send_session_entity: oneshot::Sender<Entity>,
    recv_dc: oneshot::Receiver<Disconnected>,
    recv_next: oneshot::Receiver<ToConnected>,
}

#[derive(Debug)]
struct ToConnected {
    peer_addr: SocketAddr,
    frontend: SessionFrontend,
}

fn poll_opening(
    mut commands: Commands,
    mut servers: Query<(Entity, &mut Opening), With<WebSocketServer>>,
) {
    for (entity, mut server) in &mut servers {
        if try_close(&mut commands, entity, &mut server.recv_closed) {
            continue;
        }

        let Ok(Some(next)) = server.recv_next.try_recv() else {
            continue;
        };

        let (_, dummy) = oneshot::channel();
        let recv_closed = mem::replace(&mut server.recv_closed, dummy);
        commands.entity(entity).remove::<Opening>().insert((
            Opened {
                recv_closed,
                recv_connecting: next.recv_connecting,
            },
            Server::new(Instant::now()),
            LocalAddr(next.local_addr),
        ));
    }
}

fn poll_opened(
    mut commands: Commands,
    mut servers: Query<(Entity, &mut Opened), With<WebSocketServer>>,
) {
    for (entity, mut server) in &mut servers {
        if try_close(&mut commands, entity, &mut server.recv_closed) {
            continue;
        }

        while let Ok(Some(connecting)) = server.recv_connecting.try_next() {
            let session = commands
                .spawn((
                    ChildOf(entity),
                    WebSocketServerClient(()),
                    Connecting {
                        recv_dc: connecting.recv_dc,
                        recv_next: connecting.recv_next,
                    },
                    PeerAddr(connecting.peer_addr),
                ))
                .id();
            _ = connecting.send_session_entity.send(session);
        }
    }
}

fn try_close(
    commands: &mut Commands,
    entity: Entity,
    recv_closed: &mut oneshot::Receiver<Closed>,
) -> bool {
    let closed = match recv_closed.try_recv() {
        Ok(None) => None,
        Ok(Some(closed)) => Some(closed),
        Err(_) => Some(SessionError::BackendClosed.into()),
    };
    closed.is_some_and(|closed| {
        commands.trigger_targets(closed, entity);
        true
    })
}

fn poll_connecting(
    mut commands: Commands,
    mut clients: Query<(Entity, &mut Connecting), With<WebSocketServerClient>>,
) {
    for (entity, mut client) in &mut clients {
        if try_disconnect(&mut commands, entity, &mut client.recv_dc) {
            continue;
        }

        let Ok(Some(next)) = client.recv_next.try_recv() else {
            continue;
        };

        let (_, dummy) = oneshot::channel();
        let recv_dc = mem::replace(&mut client.recv_dc, dummy);
        commands.entity(entity).remove::<Connecting>().insert((
            WebSocketIo {
                recv_packet_b2f: next.frontend.recv_packet_b2f,
                send_packet_f2b: next.frontend.send_packet_f2b,
                send_user_dc: Some(next.frontend.send_user_dc),
            },
            Connected { recv_dc },
            PeerAddr(next.peer_addr),
        ));
    }
}

fn poll_connected(
    mut commands: Commands,
    mut clients: Query<(Entity, &mut Connected), With<WebSocketServerClient>>,
) {
    for (entity, mut client) in &mut clients {
        try_disconnect(&mut commands, entity, &mut client.recv_dc);
    }
}

fn try_disconnect(
    commands: &mut Commands,
    entity: Entity,
    recv_dc: &mut oneshot::Receiver<Disconnected>,
) -> bool {
    let disconnected = match recv_dc.try_recv() {
        Ok(None) => None,
        Ok(Some(disconnected)) => Some(disconnected),
        Err(_) => Some(SessionError::BackendClosed.into()),
    };
    disconnected.is_some_and(|disconnected| {
        commands.trigger_targets(disconnected, entity);
        true
    })
}
