//! Server-side [`bevy_replicon`] support.

use {
    crate::convert,
    aeronet_io::{
        Session,
        connection::{DisconnectReason, Disconnected},
        server::{Server, ServerEndpoint},
        web_time::Instant,
    },
    aeronet_transport::{
        AeronetTransportPlugin, Transport, TransportSet,
        sampling::{SessionSamplingPlugin, SessionStats, SessionStatsSampling},
    },
    anyhow::anyhow,
    bevy_app::prelude::*,
    bevy_ecs::prelude::*,
    bevy_hierarchy::Parent,
    bevy_reflect::Reflect,
    bevy_replicon::{
        prelude::{ConnectedClients, RepliconChannels, RepliconServer},
        server::{ClientConnected, ClientDisconnected, ServerSet},
    },
    core::{any::type_name, mem},
    tracing::warn,
};

/// Provides a [`bevy_replicon`] server backend using [`Server`]s and
/// [`Session`]s for communication.
///
/// To make a [`Server`] be used by [`bevy_replicon`], add the
/// [`AeronetRepliconServer`] component.
#[derive(Debug)]
pub struct AeronetRepliconServerPlugin;

impl Plugin for AeronetRepliconServerPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<AeronetTransportPlugin>() {
            app.add_plugins(AeronetTransportPlugin);
        }
        if !app.is_plugin_added::<SessionSamplingPlugin>() {
            app.add_plugins(SessionSamplingPlugin);
        }

        app.configure_sets(
            PreUpdate,
            (
                TransportSet::Poll,
                ServerTransportSet::Poll,
                ServerSet::ReceivePackets,
            )
                .chain(),
        )
        .configure_sets(
            PostUpdate,
            (
                ServerSet::SendPackets,
                ServerTransportSet::Flush,
                TransportSet::Flush,
            )
                .chain(),
        )
        .add_systems(
            PreUpdate,
            (poll, update_state, update_client_data)
                .chain()
                .in_set(ServerTransportSet::Poll)
                .run_if(resource_exists::<RepliconServer>),
        )
        .add_systems(
            PostUpdate,
            flush
                .in_set(ServerTransportSet::Flush)
                .run_if(resource_exists::<RepliconServer>),
        )
        .add_observer(on_connected)
        .add_observer(on_disconnected);
    }
}

/// Sets for systems which provide communication between [`bevy_replicon`] and
/// [`Server`]s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum ServerTransportSet {
    /// Passing incoming messages into [`bevy_replicon`].
    ///
    /// # Ordering
    ///
    /// - [`TransportSet::Poll`]
    /// - **[`ServerTransportSet::Poll`]**
    /// - [`ServerSet::ReceivePackets`]
    Poll,
    /// Passing outgoing [`bevy_replicon`] packets to the transport layer.
    ///
    /// # Ordering
    ///
    /// - [`ServerSet::SendPackets`]
    /// - **[`ServerTransportSet::Flush`]**
    /// - [`TransportSet::Flush`]
    Flush,
}

/// Marker component for a [`Server`] which is used as the messaging backend
/// for a [`RepliconServer`].
///
/// Any server entity with this component will be used for:
/// - receiving and sending messages
///   - the child [`Entity`] (which has [`Session`]) is used as the identifier
///     for the client which sent/receives the message (see [`convert`])
/// - determining server [running] status
///   - if at least 1 entity has both [`ServerEndpoint`] and [`Server`],
///     [`RepliconServer`] is [running]
///
/// Although you can only have one [`RepliconServer`] at a time, it actually
/// makes sense to have multiple [`AeronetRepliconServer`] entities (unlike
/// with clients). This is so you can support clients from multiple different
/// types of connections - for example, if you open one server over a WebSocket
/// IO layer, and another server over a Steam networking socket IO layer,
/// clients can connect to either server, and they will both be treated as
/// connected to the [`RepliconServer`].
///
/// [`convert`]: crate::convert
/// [running]: RepliconServer::is_running
#[derive(Debug, Clone, Copy, Default, Component, Reflect)]
#[reflect(Component)]
pub struct AeronetRepliconServer;

type OpenedServer = (
    With<ServerEndpoint>,
    With<Server>,
    With<AeronetRepliconServer>,
);

fn update_state(
    mut replicon_server: ResMut<RepliconServer>,
    open_servers: Query<(), OpenedServer>,
) {
    let running = open_servers.iter().next().is_some();

    if replicon_server.is_running() != running {
        replicon_server.set_running(running);
    }
}

fn on_connected(
    trigger: Trigger<OnAdd, Session>,
    sessions: Query<&Session>,
    parents: Query<&Parent>,
    open_servers: Query<(), OpenedServer>,
    channels: Res<RepliconChannels>,
    mut commands: Commands,
) {
    let client = trigger.entity();
    let session = sessions
        .get(client)
        .expect("we are adding this component to this entity");

    let Ok(server) = parents.get(client).map(Parent::get) else {
        return;
    };
    if open_servers.get(server).is_err() {
        return;
    }

    let client_id = convert::to_client_id(client);
    commands.trigger(ClientConnected { client_id });

    let recv_lanes = channels
        .client_channels()
        .iter()
        .map(|channel| convert::to_lane_kind(channel.kind));
    let send_lanes = channels
        .server_channels()
        .iter()
        .map(|channel| convert::to_lane_kind(channel.kind));
    let transport = match Transport::new(session, recv_lanes, send_lanes, Instant::now()) {
        Ok(transport) => transport,
        Err(err) => {
            warn!("Failed to create transport for {client} connecting to {server}: {err:?}");
            return;
        }
    };

    commands.entity(client).insert(transport);
}

fn on_disconnected(
    mut trigger: Trigger<Disconnected>,
    // check for `Session` - clients which are already connected
    // on the replicon side, because if we disconnect a non-connected client,
    // replicon panics
    connected_clients: Query<&Parent, With<Session>>,
    open_servers: Query<(), OpenedServer>,
    mut commands: Commands,
) {
    let client = trigger.entity();
    let Ok(server) = connected_clients.get(client).map(Parent::get) else {
        return;
    };
    if open_servers.get(server).is_err() {
        return;
    }

    let client_id = convert::to_client_id(client);
    let reason = match &mut trigger.reason {
        DisconnectReason::User(_) => bevy_replicon::core::DisconnectReason::DisconnectedByServer,
        DisconnectReason::Peer(_) => bevy_replicon::core::DisconnectReason::DisconnectedByClient,
        DisconnectReason::Error(err) => {
            // TODO: when we can order observers, make this one run right at the end
            // so potential consumers of `Disconnected` never see this dummy error
            let err = mem::replace(
                err,
                anyhow!(
                    "real disconnect reason was replaced with a dummy value, and was passed to \
                     `bevy_replicon` - if you want to read the real disconnect reason, access it \
                     via `{}`",
                    type_name::<bevy_replicon::server::ClientDisconnected>(),
                ),
            );
            bevy_replicon::core::DisconnectReason::Backend(err.into())
        }
    };

    commands.trigger(ClientDisconnected { client_id, reason });
}

fn poll(
    mut replicon_server: ResMut<RepliconServer>,
    mut clients: Query<(Entity, &mut Transport, &Parent)>,
    open_servers: Query<(), OpenedServer>,
) {
    for (client, mut transport, server) in &mut clients {
        if open_servers.get(server.get()).is_err() {
            continue;
        }

        let client_id = convert::to_client_id(client);
        for msg in transport.recv.msgs.drain() {
            let Some(channel_id) = convert::to_channel_id(msg.lane) else {
                continue;
            };
            replicon_server.insert_received(client_id, channel_id, msg.payload);
        }

        for _ in transport.recv.acks.drain() {
            // we don't use the acks for anything
        }
    }
}

fn update_client_data(
    mut replicon_clients: ResMut<ConnectedClients>,
    clients: Query<&SessionStats>,
    sampling: Res<SessionStatsSampling>,
) {
    for client_data in replicon_clients.iter_mut() {
        let client_id = client_data.id();
        let Some(client_entity) = convert::to_entity(client_id) else {
            warn!("Attempted to update data for client {client_id:?}, which is not a valid entity");
            continue;
        };
        let Ok(stats) = clients.get(client_entity) else {
            continue;
        };

        let stats = stats.last().copied().unwrap_or_default();
        client_data.set_rtt(stats.msg_rtt.as_secs_f64());
        client_data.set_packet_loss(stats.loss);
        #[expect(clippy::cast_precision_loss, reason = "precision loss is acceptable")]
        {
            client_data.set_received_bps(stats.packets_delta.bytes_recv.0 as f64 * sampling.rate());
            client_data.set_sent_bps(stats.packets_delta.bytes_sent.0 as f64 * sampling.rate());
        }
    }
}

fn flush(mut replicon_server: ResMut<RepliconServer>, mut clients: Query<&mut Transport>) {
    let now = Instant::now();
    for (client_id, channel_id, msg) in replicon_server.drain_sent() {
        let Some(mut transport) =
            convert::to_entity(client_id).and_then(|client| clients.get_mut(client).ok())
        else {
            continue;
        };
        let lane_index = convert::to_lane_index(channel_id);

        _ = transport.send.push(lane_index, msg, now);
    }
}
