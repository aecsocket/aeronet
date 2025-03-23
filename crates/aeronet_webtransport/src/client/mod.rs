//! See [`WebTransportClient`].

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
        connection::{DisconnectReason, Disconnected},
        packet::RecvPacket,
    },
    bevy_app::prelude::*,
    bevy_ecs::{prelude::*, system::EntityCommand},
    bytes::Bytes,
    derive_more::{Display, Error, From},
    futures::channel::{mpsc, oneshot},
    tracing::{Instrument, debug_span},
    web_time::Instant,
};

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        /// Configuration for the [`WebTransportClient`] on WASM platforms.
        pub type ClientConfig = xwt_web::WebTransportOptions;

        type ConnectTarget = String;

        type ConnectError = crate::JsError;
        type AwaitConnectError = crate::JsError;
    } else {
        use wtransport::endpoint::endpoint_side;
        use xwt_core::endpoint::{Connect, connect::Connecting};

        /// Configuration for the [`WebTransportClient`] on non-WASM platforms.
        pub type ClientConfig = wtransport::ClientConfig;

        type ConnectTarget = wtransport::endpoint::ConnectOptions;
        type ClientEndpoint = xwt_wtransport::Endpoint<endpoint_side::Client>;

        type ConnectError = <ClientEndpoint as Connect>::Error;
        type AwaitConnectError = <<ClientEndpoint as Connect>::Connecting as Connecting>::Error;
    }
}

/// Allows using [`WebTransportClient`].
#[derive(Debug)]
pub struct WebTransportClientPlugin;

impl Plugin for WebTransportClientPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<WebTransportSessionPlugin>() {
            app.add_plugins(WebTransportSessionPlugin);
        }

        app.add_systems(
            PreUpdate,
            poll_clients.in_set(IoSet::Poll).before(session::poll),
        );
    }
}

/// WebTransport session implementation which acts as a dedicated client,
/// connecting to a target endpoint.
///
/// Use [`WebTransportClient::connect`] to start a connection.
#[derive(Debug, Component)]
#[require(SessionEndpoint)]
pub struct WebTransportClient(ClientFrontend);

impl WebTransportClient {
    /// Creates an [`EntityCommand`] to set up a session and connect it to the
    /// `target`.
    ///
    /// # Examples
    ///
    /// ```
    /// use {
    ///     aeronet_webtransport::client::{ClientConfig, WebTransportClient},
    ///     bevy_ecs::{prelude::*, system::EntityCommand},
    /// };
    ///
    /// # fn run(mut commands: Commands, world: &mut World) {
    /// let config = ClientConfig::default();
    /// let target = "https://[::1]:1234";
    ///
    /// // using `Commands`
    /// commands
    ///     .spawn_empty()
    ///     .queue(WebTransportClient::connect(config, target));
    ///
    /// // using mutable `World` access
    /// # let config = ClientConfig::default();
    /// let session = world.spawn_empty().id();
    /// WebTransportClient::connect(config, target).apply(session, world);
    /// # }
    /// ```
    #[must_use]
    pub fn connect(
        config: impl Into<ClientConfig>,
        #[cfg(target_family = "wasm")] target: impl Into<String>,
        #[cfg(not(target_family = "wasm"))] target: impl wtransport::endpoint::IntoConnectOptions,
    ) -> impl EntityCommand {
        let config = config.into();
        let target = {
            #[cfg(target_family = "wasm")]
            {
                target.into()
            }

            #[cfg(not(target_family = "wasm"))]
            {
                target.into_options()
            }
        };
        move |entity: EntityWorldMut| connect(entity, config, target)
    }
}

fn connect(mut entity: EntityWorldMut, config: ClientConfig, target: ConnectTarget) {
    let runtime = entity.world().resource::<WebTransportRuntime>().clone();
    let (send_dc, recv_dc) = oneshot::channel::<DisconnectReason<ClientError>>();
    let (send_next, recv_next) = oneshot::channel::<ToConnected>();
    runtime.spawn_on_self(
        async move {
            let Err(reason) = backend::start(config, target, send_next).await;
            _ = send_dc.send(reason);
        }
        .instrument(debug_span!("client", entity = %entity.id())),
    );

    entity.insert(WebTransportClient(ClientFrontend::Connecting {
        recv_dc,
        recv_next,
    }));
}

/// [`WebTransportClient`] error.
#[derive(Debug, Display, Error, From)]
#[non_exhaustive]
pub enum ClientError {
    /// Failed to start connecting to the target.
    #[display("failed to connect")]
    Connect(ConnectError),
    /// Failed to await the connection to the target.
    #[display("failed to await connection")]
    AwaitConnect(AwaitConnectError),
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
    initial_peer_addr: core::net::SocketAddr,
    #[cfg(not(target_family = "wasm"))]
    initial_rtt: core::time::Duration,
    initial_mtu: usize,
    recv_meta: mpsc::Receiver<SessionMeta>,
    recv_packet_b2f: mpsc::UnboundedReceiver<RecvPacket>,
    send_packet_f2b: mpsc::UnboundedSender<Bytes>,
    send_user_dc: oneshot::Sender<String>,
}

fn poll_clients(mut commands: Commands, mut frontends: Query<(Entity, &mut WebTransportClient)>) {
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
    entity: Entity,
    mut recv_dc: oneshot::Receiver<DisconnectReason<ClientError>>,
    mut recv_next: oneshot::Receiver<ToConnected>,
) -> ClientFrontend {
    if should_disconnect(commands, entity, &mut recv_dc) {
        return ClientFrontend::Disconnected;
    }

    let Ok(Some(next)) = recv_next.try_recv() else {
        return ClientFrontend::Connecting { recv_dc, recv_next };
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
        #[cfg(not(target_family = "wasm"))]
        aeronet_io::connection::LocalAddr(next.local_addr),
        #[cfg(not(target_family = "wasm"))]
        aeronet_io::connection::PeerAddr(next.initial_peer_addr),
        #[cfg(not(target_family = "wasm"))]
        aeronet_io::packet::PacketRtt(next.initial_rtt),
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
        commands.trigger_targets(
            Disconnected {
                reason: reason.map_err(anyhow::Error::new),
            },
            session,
        );
        true
    })
}
