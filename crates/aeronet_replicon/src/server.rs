//! Server-side [`bevy_replicon`] support.

use {
    crate::convert,
    aeronet_io::{
        Session,
        server::{Server, ServerEndpoint},
    },
    aeronet_transport::{
        AeronetTransportPlugin, Transport, TransportSystems,
        sampling::{SessionSamplingPlugin, SessionStats, SessionStatsSampling},
    },
    bevy_app::prelude::*,
    bevy_ecs::prelude::*,
    bevy_platform::time::Instant,
    bevy_reflect::Reflect,
    bevy_replicon::{prelude::*, server::ServerSystems},
    bevy_state::state::NextState,
    log::warn,
};

/// Provides a [`bevy_replicon`] server backend using [`Server`]s and
/// [`Session`]s for communication.
///
/// To make a [`Server`] be used by [`bevy_replicon`], add the
/// [`AeronetRepliconServer`] component.
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
                TransportSystems::Poll,
                ServerTransportSystems::Poll,
                ServerSystems::ReceivePackets,
            )
                .chain(),
        )
        .configure_sets(
            PostUpdate,
            (
                ServerSystems::SendPackets,
                ServerTransportSystems::Flush,
                TransportSystems::Flush,
            )
                .chain(),
        )
        .add_systems(
            PreUpdate,
            (poll, update_state, update_client_data)
                .chain()
                .in_set(ServerTransportSystems::Poll)
                .run_if(resource_exists::<ServerMessages>),
        )
        .add_systems(
            PostUpdate,
            (flush, handle_disconnect_requests)
                .in_set(ServerTransportSystems::Flush)
                .run_if(resource_exists::<ServerMessages>),
        )
        .add_observer(on_connected);
    }
}

/// Sets for systems which provide communication between [`bevy_replicon`] and
/// [`Server`]s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum ServerTransportSystems {
    /// Passing incoming messages into [`bevy_replicon`].
    ///
    /// # Ordering
    ///
    /// - [`TransportSystems::Poll`]
    /// - **[`ServerTransportSystems::Poll`]**
    /// - [`ServerSystems::ReceivePackets`]
    Poll,
    /// Passing outgoing [`bevy_replicon`] packets to the transport layer.
    ///
    /// # Ordering
    ///
    /// - [`ServerSystems::SendPackets`]
    /// - **[`ServerTransportSystems::Flush`]**
    /// - [`TransportSystems::Flush`]
    Flush,
}

/// Marker component for a [`Server`] which is used as the messaging backend
/// for a Replicon server.
///
/// Any server entity with this component will be used for:
/// - receiving and sending messages
///   - the child [`Entity`] (which has [`Session`]) is used as the identifier
///     for the client which sent/receives the message (see [`convert`])
/// - determining server [running] status
///   - if at least 1 entity has both [`ServerEndpoint`] and [`Server`],
///     [`ServerState`] is [running]
///
/// Although you can only have one Replicon server at a time, it actually
/// makes sense to have multiple [`AeronetRepliconServer`] entities (unlike
/// with clients). This is so you can support clients from multiple different
/// types of connections - for example, if you open one server over a WebSocket
/// IO layer, and another server over a Steam networking socket IO layer,
/// clients can connect to either server, and they will both be treated as
/// connected to the Replicon server.
///
/// [`convert`]: crate::convert
/// [running]: ServerState::Running
#[derive(Debug, Clone, Copy, Default, Component, Reflect)]
#[reflect(Component)]
pub struct AeronetRepliconServer;

type OpenedServer = (
    With<ServerEndpoint>,
    With<Server>,
    With<AeronetRepliconServer>,
);

fn update_state(
    mut next_server_state: ResMut<NextState<ServerState>>,
    open_servers: Query<(), OpenedServer>,
) {
    let running = open_servers.iter().next().is_some();
    let next_status = if running {
        ServerState::Running
    } else {
        ServerState::Stopped
    };
    next_server_state.set(next_status);
}

fn on_connected(
    trigger: On<Add, Session>,
    sessions: Query<&Session>,
    child_of: Query<&ChildOf>,
    open_servers: Query<(), OpenedServer>,
    channels: Res<RepliconChannels>,
    mut commands: Commands,
) {
    let client = trigger.event_target();
    let session = sessions
        .get(client)
        .expect("we are adding this component to this entity");

    let Ok(&ChildOf(server)) = child_of.get(client) else {
        return;
    };
    if open_servers.get(server).is_err() {
        return;
    }

    let rx_lanes = channels
        .client_channels()
        .iter()
        .map(|channel| convert::to_lane_kind(*channel));
    let tx_lanes = channels
        .server_channels()
        .iter()
        .map(|channel| convert::to_lane_kind(*channel));
    let transport = match Transport::new(session, rx_lanes, tx_lanes, Instant::now()) {
        Ok(transport) => transport,
        Err(err) => {
            warn!("Failed to create transport for {client} connecting to {server}: {err:?}");
            return;
        }
    };

    commands.entity(client).insert((
        ConnectedClient {
            max_size: session.mtu(),
        },
        transport,
    ));
}

fn poll(
    mut server_msgs: ResMut<ServerMessages>,
    mut clients: Query<(Entity, &mut Transport, &ChildOf)>,
    open_servers: Query<(), OpenedServer>,
) {
    for (client, mut transport, &ChildOf(server)) in &mut clients {
        if open_servers.get(server).is_err() {
            continue;
        }

        for msg in transport.recv.msgs.drain() {
            let channel_id = convert::to_channel_id(msg.lane);
            server_msgs.insert_received(client, channel_id, msg.payload);
        }

        for _ in transport.recv.acks.drain() {
            // we don't use the acks for anything
        }
    }
}

fn update_client_data(
    mut clients: Query<(
        &Session,
        &SessionStats,
        &mut ConnectedClient,
        &mut ClientStats,
    )>,
    sampling: Res<SessionStatsSampling>,
) {
    for (session, session_stats, mut connected_client, mut client_stats) in &mut clients {
        let stats = session_stats.last().copied().unwrap_or_default();
        connected_client.max_size = session.mtu();
        client_stats.rtt = stats.msg_rtt.as_secs_f64();
        client_stats.packet_loss = stats.loss;
        #[expect(clippy::cast_precision_loss, reason = "precision loss is acceptable")]
        {
            client_stats.received_bps = stats.packets_delta.bytes_recv.0 as f64 * sampling.rate();
            client_stats.sent_bps = stats.packets_delta.bytes_sent.0 as f64 * sampling.rate();
        }
    }
}

fn flush(mut server_msgs: ResMut<ServerMessages>, mut clients: Query<&mut Transport>) {
    let now = Instant::now();
    for (client, channel_id, msg) in server_msgs.drain_sent() {
        let Ok(mut transport) = clients.get_mut(client) else {
            warn!("Sending to non-existent client {client}");
            continue;
        };
        let Some(lane_index) = convert::to_lane_index(channel_id) else {
            warn!("Channel {channel_id} is too large to convert to a lane index");
            continue;
        };

        _ = transport.send.push(lane_index, msg, now);
    }
}

fn handle_disconnect_requests(
    mut commands: Commands,
    mut requests: MessageReader<DisconnectRequest>,
    clients: Query<(), (With<Transport>, With<ChildOf>)>,
) {
    for request in requests.read() {
        let client = request.client;
        if let Err(err) = clients.get(client) {
            warn!("Requested to disconnect {client} which does not match query: {err:?}");
            continue;
        }

        commands.entity(client).despawn();
    }
}
