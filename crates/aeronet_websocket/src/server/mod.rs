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
        IoSystems, SessionEndpoint,
        connection::{DisconnectReason, Disconnected, LocalAddr, PeerAddr},
        server::{CloseReason, Closed, Server, ServerEndpoint},
    },
    bevy_app::prelude::*,
    bevy_ecs::prelude::*,
    bevy_platform::time::Instant,
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
                .in_set(IoSystems::Poll)
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
        WebSocketServer(()),
        Opening {
            rx_close_reason,
            rx_next,
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
    peer_addr: SocketAddr,
    tx_session_entity: oneshot::Sender<Entity>,
    rx_dc_reason: oneshot::Receiver<DisconnectReason>,
    rx_next: oneshot::Receiver<ToConnected>,
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
        if try_close(&mut commands, entity, &mut server.rx_close_reason) {
            continue;
        }

        let Ok(Some(next)) = server.rx_next.try_recv() else {
            continue;
        };

        let (_, dummy) = oneshot::channel();
        let rx_close_reason = mem::replace(&mut server.rx_close_reason, dummy);
        commands.entity(entity).remove::<Opening>().insert((
            Opened {
                rx_close_reason,
                rx_connecting: next.rx_connecting,
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
        if try_close(&mut commands, entity, &mut server.rx_close_reason) {
            continue;
        }

        while let Ok(Some(connecting)) = server.rx_connecting.try_next() {
            let session = commands
                .spawn((
                    ChildOf(entity),
                    WebSocketServerClient(()),
                    Connecting {
                        rx_dc_reason: connecting.rx_dc_reason,
                        rx_next: connecting.rx_next,
                    },
                    PeerAddr(connecting.peer_addr),
                ))
                .id();
            _ = connecting.tx_session_entity.send(session);
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
    mut clients: Query<(Entity, &mut Connecting), With<WebSocketServerClient>>,
) {
    for (client, mut client_io) in &mut clients {
        if try_disconnect(&mut commands, client, &mut client_io.rx_dc_reason) {
            continue;
        }

        let Ok(Some(next)) = client_io.rx_next.try_recv() else {
            continue;
        };

        let (_, dummy) = oneshot::channel();
        let rx_dc_reason = mem::replace(&mut client_io.rx_dc_reason, dummy);
        commands.entity(client).remove::<Connecting>().insert((
            WebSocketIo {
                rx_packet_b2f: next.frontend.rx_packet_b2f,
                tx_packet_f2b: next.frontend.tx_packet_f2b,
                tx_user_dc: Some(next.frontend.tx_user_dc),
            },
            Connected { rx_dc_reason },
            PeerAddr(next.peer_addr),
        ));
    }
}

fn poll_connected(
    mut commands: Commands,
    mut clients: Query<(Entity, &mut Connected), With<WebSocketServerClient>>,
) {
    for (entity, mut client) in &mut clients {
        try_disconnect(&mut commands, entity, &mut client.rx_dc_reason);
    }
}

fn try_disconnect(
    commands: &mut Commands,
    entity: Entity,
    rx_dc_reason: &mut oneshot::Receiver<DisconnectReason>,
) -> bool {
    let dc_reason = match rx_dc_reason.try_recv() {
        Ok(None) => None,
        Ok(Some(disconnected)) => Some(disconnected),
        Err(_) => Some(SessionError::BackendClosed.into()),
    };
    dc_reason.is_some_and(|reason| {
        commands.trigger(Disconnected { entity, reason });
        true
    })
}
