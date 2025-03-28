//! See [`WebTransportClient`].

mod backend;

use {
    crate::{
        runtime::WebTransportRuntime,
        session::{
            self, MIN_MTU, SessionError, SessionMeta, WebTransportIo, WebTransportSessionPlugin,
        },
    },
    aeronet_io::{IoSet, Session, SessionEndpoint, connection::Disconnected, packet::RecvPacket},
    bevy_app::prelude::*,
    bevy_ecs::{prelude::*, system::EntityCommand},
    bevy_platform_support::time::Instant,
    bytes::Bytes,
    core::mem,
    derive_more::{Display, Error},
    futures::channel::{mpsc, oneshot},
    tracing::{Instrument, debug, debug_span},
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
        use xwt_core::endpoint::{Connect as XwtConnect, connect::Connecting as XwtConnecting};

        /// Configuration for the [`WebTransportClient`] on non-WASM platforms.
        pub type ClientConfig = wtransport::ClientConfig;

        type ConnectTarget = wtransport::endpoint::ConnectOptions;
        type ClientEndpoint = xwt_wtransport::Endpoint<endpoint_side::Client>;

        type ConnectError = <ClientEndpoint as XwtConnect>::Error;
        type AwaitConnectError = <<ClientEndpoint as XwtConnect>::Connecting as XwtConnecting>::Error;
    }
}

/// Allows using [`WebTransportClient`].
pub struct WebTransportClientPlugin;

impl Plugin for WebTransportClientPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<WebTransportSessionPlugin>() {
            app.add_plugins(WebTransportSessionPlugin);
        }

        app.add_systems(
            PreUpdate,
            (poll_connecting, poll_connected)
                .in_set(IoSet::Poll)
                .before(session::poll),
        );
    }
}

/// WebTransport session implementation which acts as a dedicated client,
/// connecting to a target endpoint.
///
/// Use [`WebTransportClient::connect`] to start a connection.
#[derive(Debug, Component)]
#[require(SessionEndpoint)]
pub struct WebTransportClient(());

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
    /// # let config: ClientConfig = unreachable!();
    /// let client = world.spawn_empty().id();
    /// WebTransportClient::connect(config, target).apply(world.entity_mut(client));
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

    entity.insert((WebTransportClient(()), Connecting { recv_dc, recv_next }));
}

/// [`WebTransportClient`]-specific error.
///
/// For generic WebTransport errors, see [`SessionError`].
#[derive(Debug, Display, Error)]
#[non_exhaustive]
pub enum ClientError {
    /// Failed to start connecting to the target.
    #[display("failed to connect")]
    Connect(ConnectError),
    /// Failed to await the connection to the target.
    #[display("failed to await connection")]
    AwaitConnect(AwaitConnectError),
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
    initial_peer_addr: core::net::SocketAddr,
    #[cfg(not(target_family = "wasm"))]
    initial_rtt: core::time::Duration,
    initial_mtu: usize,
    recv_meta: mpsc::Receiver<SessionMeta>,
    recv_packet_b2f: mpsc::UnboundedReceiver<RecvPacket>,
    send_packet_f2b: mpsc::UnboundedSender<Bytes>,
    send_user_dc: oneshot::Sender<String>,
}

fn poll_connecting(
    mut commands: Commands,
    mut clients: Query<(Entity, &mut Connecting), With<WebTransportClient>>,
) {
    for (entity, mut client) in &mut clients {
        if try_disconnect(&mut commands, entity, &mut client.recv_dc) {
            continue;
        }

        let Ok(Some(next)) = client.recv_next.try_recv() else {
            continue;
        };

        let mut session = Session::new(Instant::now(), MIN_MTU);
        if let Err(err) = session.set_mtu(next.initial_mtu) {
            commands.trigger_targets(
                Disconnected::by_error(SessionError::MtuTooSmall(err)),
                entity,
            );
            continue;
        }

        let (_, dummy) = oneshot::channel();
        let recv_dc = mem::replace(&mut client.recv_dc, dummy);
        commands.entity(entity).remove::<Connecting>().insert((
            WebTransportIo {
                recv_meta: next.recv_meta,
                recv_packet_b2f: next.recv_packet_b2f,
                send_packet_f2b: next.send_packet_f2b,
                send_user_dc: Some(next.send_user_dc),
            },
            Connected { recv_dc },
            session,
            #[cfg(not(target_family = "wasm"))]
            aeronet_io::connection::LocalAddr(next.local_addr),
            #[cfg(not(target_family = "wasm"))]
            aeronet_io::connection::PeerAddr(next.initial_peer_addr),
            #[cfg(not(target_family = "wasm"))]
            aeronet_io::packet::PacketRtt(next.initial_rtt),
        ));
    }
}

fn poll_connected(
    mut commands: Commands,
    mut clients: Query<(Entity, &mut Connected), With<WebTransportClient>>,
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
