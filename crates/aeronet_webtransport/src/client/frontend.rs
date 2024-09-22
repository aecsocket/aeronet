use {
    super::{backend, ClientConfig, ClientError, ToConnected},
    crate::{
        runtime::WebTransportRuntime,
        session::{SessionError, WebTransportIo, WebTransportSessionPlugin},
    },
    aeronet_io::{
        connection::{DisconnectReason, Disconnected, LocalAddr, RemoteAddr, Session},
        packet::{PacketBuffersCapacity, PacketMtu, PacketRtt},
        IoSet,
    },
    bevy_app::prelude::*,
    bevy_ecs::{prelude::*, system::EntityCommand},
    futures::channel::oneshot,
    tracing::{debug_span, Instrument},
};

/// Allows using [`WebTransportClient`].
#[derive(Debug)]
pub struct WebTransportClientPlugin;

impl Plugin for WebTransportClientPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<WebTransportSessionPlugin>() {
            app.add_plugins(WebTransportSessionPlugin);
        }

        app.add_systems(PreUpdate, poll_clients.before(IoSet::Poll))
            .observe(on_client_added);
    }
}

/// WebTransport session implementation which acts as a dedicated client,
/// connecting to a target endpoint.
///
/// Use [`WebTransportClient::connect`] to start a connection.
#[derive(Debug, Component)]
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
    ///     bevy_ecs::prelude::*,
    /// };
    ///
    /// # fn run(mut commands: Commands, world: &mut World) {
    /// let config = ClientConfig::default();
    /// let target = "https://[::1]:1234";
    ///
    /// // using `Commands`
    /// commands
    ///     .spawn_empty()
    ///     .add(WebTransportClient::connect(config, target));
    ///
    /// // using mutable `World` access
    /// # let config = ClientConfig::default();
    /// let session = world.spawn_empty().id();
    /// WebTransportClient::connect(config, target).apply(session, world);
    /// # }
    /// ```
    #[must_use]
    pub fn connect(config: ClientConfig, target: impl Into<String>) -> impl EntityCommand {
        let target = target.into();
        |session: Entity, world: &mut World| connect(session, world, config, target)
    }
}

fn connect(session: Entity, world: &mut World, config: ClientConfig, target: String) {
    let runtime = world.resource::<WebTransportRuntime>().clone();
    let packet_buf_cap = PacketBuffersCapacity::compute_from(world, session);

    let (send_dc, recv_dc) = oneshot::channel::<DisconnectReason<ClientError>>();
    let (send_next, recv_next) = oneshot::channel::<ToConnected>();
    runtime.spawn({
        let runtime = runtime.clone();
        async move {
            let Err(reason) =
                backend::start(runtime, packet_buf_cap, config, target, send_next).await
            else {
                unreachable!();
            };
            let _ = send_dc.send(reason);
        }
        .instrument(debug_span!("client", %session))
    });

    world
        .entity_mut(session)
        .insert(WebTransportClient(ClientFrontend::Connecting {
            recv_dc,
            recv_next,
        }));
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

// TODO: required components
fn on_client_added(trigger: Trigger<OnAdd, WebTransportClient>, mut commands: Commands) {
    let session = trigger.entity();
    commands.entity(session).insert(Session);
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
        WebTransportIo {
            recv_meta: next.recv_meta,
            recv_packet_b2f: next.recv_packet_b2f,
            send_packet_f2b: next.send_packet_f2b,
            send_user_dc: Some(next.send_user_dc),
        },
        PacketMtu(next.initial_mtu),
        #[cfg(not(target_family = "wasm"))]
        LocalAddr(next.local_addr),
        #[cfg(not(target_family = "wasm"))]
        RemoteAddr(next.initial_remote_addr),
        #[cfg(not(target_family = "wasm"))]
        PacketRtt(next.initial_rtt),
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
    dc_reason.map_or(false, |reason| {
        let reason = reason.map_err(anyhow::Error::new);
        commands.trigger_targets(Disconnected { reason }, session);
        true
    })
}
