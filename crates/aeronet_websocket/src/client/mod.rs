//! See [`WebSocketClient`].

mod backend;

use {
    crate::{
        WebSocketRuntime,
        session::{self, MTU, SessionError, SessionFrontend, WebSocketIo, WebSocketSessionPlugin},
    },
    aeronet_io::{IoSet, Session, SessionEndpoint, connection::Disconnected},
    bevy_app::prelude::*,
    bevy_ecs::prelude::*,
    bevy_platform_support::time::Instant,
    core::mem,
    derive_more::{Display, Error},
    futures::{channel::oneshot, never::Never},
    tracing::{Instrument, debug, debug_span},
};

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        type ConnectTarget = String;

        type CreateTargetError = Never;
        type CreateSocketError = crate::JsError;
        type ConnectError = crate::JsError;

        /// Dummy configuration type for [`WebSocketClient`], used to keep
        /// parity between native and WASM APIs.
        #[derive(Debug, Clone, Default)]
        pub struct ClientConfig;
    } else {
        mod config;
        pub use config::*;

        use crate::tungstenite;

        type ConnectTarget = Result<tungstenite::handshake::client::Request, tungstenite::Error>;

        type CreateTargetError = tungstenite::Error;
        type CreateSocketError = Never;
        type ConnectError = tungstenite::Error;
    }
}

/// Allows using [`WebSocketClient`].
pub struct WebSocketClientPlugin;

impl Plugin for WebSocketClientPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<WebSocketSessionPlugin>() {
            app.add_plugins(WebSocketSessionPlugin);
        }

        app.add_systems(
            PreUpdate,
            (poll_connecting, poll_connected)
                .in_set(IoSet::Poll)
                .before(session::poll),
        );
    }
}

/// WebSocket session implementation which acts as a dedicated client,
/// connecting to a target socket.
///
/// Use [`WebSocketClient::connect`] to start a connection.
#[derive(Debug, Component)]
#[require(SessionEndpoint)]
pub struct WebSocketClient(());

impl WebSocketClient {
    /// Creates an [`EntityCommand`] to set up a session and connect it to the
    /// `target`.
    ///
    /// # Examples
    ///
    /// ```
    /// use {
    ///     aeronet_websocket::client::{ClientConfig, WebSocketClient},
    ///     bevy_ecs::prelude::*,
    /// };
    ///
    /// # fn run(mut commands: Commands, world: &mut World) {
    /// let config = ClientConfig::default();
    /// let target = "wss://[::1]:1234";
    ///
    /// // using `Commands`
    /// commands
    ///     .spawn_empty()
    ///     .queue(WebSocketClient::connect(config, target));
    ///
    /// // using mutable `World` access
    /// # let config: ClientConfig = unreachable!();
    /// let session = world.spawn_empty().id();
    /// WebSocketClient::connect(config, target).apply(world.entity_mut(session));
    /// # }
    /// ```
    #[must_use]
    pub fn connect(
        config: impl Into<ClientConfig>,
        #[cfg(target_family = "wasm")] target: impl Into<String>,
        #[cfg(not(target_family = "wasm"))] target: impl tungstenite::client::IntoClientRequest,
    ) -> impl EntityCommand {
        let config = config.into();
        let target = {
            #[cfg(target_family = "wasm")]
            {
                target.into()
            }

            #[cfg(not(target_family = "wasm"))]
            {
                target.into_client_request()
            }
        };
        move |entity: EntityWorldMut| connect(entity, config, target)
    }
}

fn connect(mut entity: EntityWorldMut, config: ClientConfig, target: ConnectTarget) {
    let runtime = entity.world().resource::<WebSocketRuntime>().clone();

    let (send_dc, recv_dc) = oneshot::channel::<Disconnected>();
    let (send_next, recv_next) = oneshot::channel::<ToConnected>();
    runtime.spawn_on_self(
        async move {
            let Err(disconnected) = backend::start(config, target, send_next).await;
            debug!("Client disconnected: {disconnected:?}");
            _ = send_dc.send(disconnected);
        }
        .instrument(debug_span!("client", entity = %entity.id())),
    );

    entity.insert((WebSocketClient(()), Connecting { recv_dc, recv_next }));
}

/// [`WebSocketClient`]-specific error.
///
/// For generic WebSocket errors, see [`SessionError`].
#[derive(Debug, Display, Error)]
#[non_exhaustive]
pub enum ClientError {
    /// Failed to convert the `target` passed into [`WebSocketClient::connect`]
    /// into an actual connection target.
    #[display("failed to create request target")]
    CreateTarget(CreateTargetError),
    /// Failed to create the socket used for connecting.
    #[display("failed to create socket")]
    CreateSocket(CreateSocketError),
    /// Failed to connect to the target.
    #[display("failed to connect")]
    Connect(ConnectError),
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
struct ToConnected {
    #[cfg(not(target_family = "wasm"))]
    local_addr: core::net::SocketAddr,
    #[cfg(not(target_family = "wasm"))]
    peer_addr: core::net::SocketAddr,
    frontend: SessionFrontend,
}

fn poll_connecting(
    mut commands: Commands,
    mut clients: Query<(Entity, &mut Connecting), With<WebSocketClient>>,
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
            Session::new(Instant::now(), MTU),
            #[cfg(not(target_family = "wasm"))]
            aeronet_io::connection::LocalAddr(next.local_addr),
            #[cfg(not(target_family = "wasm"))]
            aeronet_io::connection::PeerAddr(next.peer_addr),
        ));
    }
}

fn poll_connected(
    mut commands: Commands,
    mut clients: Query<(Entity, &mut Connected), With<WebSocketClient>>,
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
