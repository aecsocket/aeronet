//! See [`WebSocketClient`].

mod backend;

use {
    crate::{
        WebSocketRuntime,
        session::{self, MTU, SessionError, SessionFrontend, WebSocketIo, WebSocketSessionPlugin},
    },
    aeronet_io::{
        IoSystems, Session, SessionEndpoint,
        connection::{DisconnectReason, Disconnected},
    },
    bevy_app::prelude::*,
    bevy_ecs::prelude::*,
    bevy_platform::time::Instant,
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
                .in_set(IoSystems::Poll)
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

    let (tx_dc_reason, rx_dc_reason) = oneshot::channel::<DisconnectReason>();
    let (tx_next, rx_next) = oneshot::channel::<ToConnected>();
    runtime.spawn_on_self(
        async move {
            let Err(dc_reason) = backend::start(config, target, tx_next).await;
            debug!("Client disconnected: {dc_reason:?}");
            _ = tx_dc_reason.send(dc_reason);
        }
        .instrument(debug_span!("client", entity = %entity.id())),
    );

    entity.insert((
        WebSocketClient(()),
        Connecting {
            rx_dc_reason,
            rx_next,
        },
    ));
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
    rx_dc_reason: oneshot::Receiver<DisconnectReason>,
    rx_next: oneshot::Receiver<ToConnected>,
}

#[derive(Debug, Component)]
struct Connected {
    rx_dc_reason: oneshot::Receiver<DisconnectReason>,
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
        if try_disconnect(&mut commands, entity, &mut client.rx_dc_reason) {
            continue;
        }

        let Ok(Some(next)) = client.rx_next.try_recv() else {
            continue;
        };

        let (_, dummy) = oneshot::channel();
        let rx_dc_reason = mem::replace(&mut client.rx_dc_reason, dummy);
        commands.entity(entity).remove::<Connecting>().insert((
            WebSocketIo {
                rx_packet_b2f: next.frontend.rx_packet_b2f,
                tx_packet_f2b: next.frontend.tx_packet_f2b,
                tx_user_dc: Some(next.frontend.tx_user_dc),
            },
            Connected { rx_dc_reason },
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
    for (client, mut client_io) in &mut clients {
        try_disconnect(&mut commands, client, &mut client_io.rx_dc_reason);
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
