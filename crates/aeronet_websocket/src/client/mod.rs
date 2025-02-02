//! See [`WebSocketClient`].

mod backend;

use {
    crate::{
        WebSocketRuntime,
        session::{self, MTU, SessionError, SessionFrontend, WebSocketIo, WebSocketSessionPlugin},
    },
    aeronet_io::{
        IoSet, Session, SessionEndpoint,
        connection::{DisconnectReason, Disconnected},
    },
    bevy_app::prelude::*,
    bevy_ecs::{prelude::*, system::EntityCommand},
    derive_more::{Display, Error, From},
    futures::{channel::oneshot, never::Never},
    tracing::{Instrument, debug_span},
    web_time::Instant,
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
#[derive(Debug)]
pub struct WebSocketClientPlugin;

impl Plugin for WebSocketClientPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<WebSocketSessionPlugin>() {
            app.add_plugins(WebSocketSessionPlugin);
        }

        app.add_systems(
            PreUpdate,
            poll_clients.in_set(IoSet::Poll).before(session::poll),
        );
    }
}

/// WebSocket session implementation which acts as a dedicated client,
/// connecting to a target socket.
///
/// Use [`WebSocketClient::connect`] to start a connection.
#[derive(Debug, Component)]
#[require(SessionEndpoint)]
pub struct WebSocketClient(ClientFrontend);

impl WebSocketClient {
    /// Creates an [`EntityCommand`] to set up a session and connect it to the
    /// `target`.
    ///
    /// # Examples
    ///
    /// ```
    /// use {
    ///     aeronet_websocket::client::{ClientConfig, WebSocketClient},
    ///     bevy_ecs::{prelude::*, system::EntityCommand},
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
    /// # let config = ClientConfig::default();
    /// let session = world.spawn_empty().id();
    /// WebSocketClient::connect(config, target).apply(session, world);
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
        move |session: Entity, world: &mut World| connect(session, world, config, target)
    }
}

fn connect(session: Entity, world: &mut World, config: ClientConfig, target: ConnectTarget) {
    let runtime = world.resource::<WebSocketRuntime>().clone();

    let (send_dc, recv_dc) = oneshot::channel::<DisconnectReason<ClientError>>();
    let (send_next, recv_next) = oneshot::channel::<ToConnected>();
    runtime.spawn_on_self(
        async move {
            let Err(reason) = backend::start(config, target, send_next).await;
            _ = send_dc.send(reason);
        }
        .instrument(debug_span!("client", %session)),
    );

    world
        .entity_mut(session)
        .insert(WebSocketClient(ClientFrontend::Connecting {
            recv_dc,
            recv_next,
        }));
}

/// [`WebSocketClient`] error.
#[derive(Debug, Display, Error, From)]
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
    /// Generic session error.
    #[from]
    Session(SessionError),
}

#[derive(Debug)]
enum ClientFrontend {
    Connecting {
        recv_dc: oneshot::Receiver<DisconnectReason<ClientError>>,
        recv_next: oneshot::Receiver<ToConnected>,
    },
    Connected {
        recv_dc: oneshot::Receiver<DisconnectReason<ClientError>>,
    },
    Disconnected,
}

#[derive(Debug)]
struct ToConnected {
    #[cfg(not(target_family = "wasm"))]
    local_addr: core::net::SocketAddr,
    #[cfg(not(target_family = "wasm"))]
    peer_addr: core::net::SocketAddr,
    frontend: SessionFrontend,
}

fn poll_clients(mut commands: Commands, mut frontends: Query<(Entity, &mut WebSocketClient)>) {
    for (session, mut frontend) in &mut frontends {
        replace_with::replace_with_or_abort(&mut frontend.0, |state| match state {
            ClientFrontend::Connecting { recv_dc, recv_next } => {
                poll_connecting(&mut commands, session, recv_dc, recv_next)
            }
            ClientFrontend::Connected { mut recv_dc } => {
                if should_disconnect(&mut commands, session, &mut recv_dc) {
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
    session: Entity,
    mut recv_dc: oneshot::Receiver<DisconnectReason<ClientError>>,
    mut recv_next: oneshot::Receiver<ToConnected>,
) -> ClientFrontend {
    if should_disconnect(commands, session, &mut recv_dc) {
        return ClientFrontend::Disconnected;
    }

    let Ok(Some(next)) = recv_next.try_recv() else {
        return ClientFrontend::Connecting { recv_dc, recv_next };
    };

    commands.entity(session).insert((
        WebSocketIo {
            recv_packet_b2f: next.frontend.recv_packet_b2f,
            send_packet_f2b: next.frontend.send_packet_f2b,
            send_user_dc: Some(next.frontend.send_user_dc),
        },
        Session::new(Instant::now(), MTU),
        #[cfg(not(target_family = "wasm"))]
        aeronet_io::connection::LocalAddr(next.local_addr),
        #[cfg(not(target_family = "wasm"))]
        aeronet_io::connection::PeerAddr(next.peer_addr),
    ));
    ClientFrontend::Connected { recv_dc }
}

fn should_disconnect(
    commands: &mut Commands,
    session: Entity,
    recv_dc: &mut oneshot::Receiver<DisconnectReason<ClientError>>,
) -> bool {
    let dc_reason = match recv_dc.try_recv() {
        Ok(None) => None,
        Ok(Some(dc_reason)) => Some(dc_reason),
        Err(_) => Some(ClientError::Session(SessionError::BackendClosed).into()),
    };
    dc_reason.is_some_and(|reason| {
        let reason = reason.map_err(anyhow::Error::new);
        commands.trigger_targets(Disconnected { reason }, session);
        true
    })
}
