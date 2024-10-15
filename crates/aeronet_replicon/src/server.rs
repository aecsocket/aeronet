//! Server-side [`bevy_replicon`] support.

use {
    crate::convert::{IntoClientId, IntoLaneIndex, TryIntoChannelId, TryIntoEntity},
    aeronet_io::{
        connection::{Connected, DisconnectReason, Disconnected},
        server::{Opened, Server},
    },
    aeronet_transport::{message::MessageBuffers, AeronetTransportPlugin, Transport, TransportSet},
    bevy_app::prelude::*,
    bevy_ecs::prelude::*,
    bevy_hierarchy::Parent,
    bevy_reflect::Reflect,
    bevy_replicon::{
        prelude::RepliconServer,
        server::{ServerEvent, ServerSet},
    },
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
            (poll, update_state)
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
        .observe(on_connected)
        .observe(on_disconnected);
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
///   - if at least 1 server is [`Opened`], [`RepliconServer`] is [running]
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

type OpenedServer = (With<Server>, With<Opened>, With<AeronetRepliconServer>);

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
    trigger: Trigger<OnAdd, Connected>,
    clients: Query<&Parent>,
    open_servers: Query<(), OpenedServer>,
    mut events: EventWriter<ServerEvent>,
    mut commands: Commands,
) {
    let client = trigger.entity();
    let Ok(server) = clients.get(client).map(Parent::get) else {
        return;
    };
    if open_servers.get(server).is_err() {
        return;
    }

    let client_id = client.into_client_id();
    events.send(ServerEvent::ClientConnected { client_id });
    commands.entity(client).insert(Transport);
}

fn on_disconnected(
    trigger: Trigger<Disconnected>,
    clients: Query<&Parent, With<Connected>>,
    open_servers: Query<(), OpenedServer>,
    mut events: EventWriter<ServerEvent>,
) {
    let client = trigger.entity();
    let Ok(server) = clients.get(client).map(Parent::get) else {
        return;
    };
    if open_servers.get(server).is_err() {
        return;
    }

    let client_id = client.into_client_id();
    let reason = match &**trigger.event() {
        DisconnectReason::User(reason) | DisconnectReason::Peer(reason) => reason.clone(),
        DisconnectReason::Error(err) => format!("{err:#}"),
    };
    // only disconnect already-connected clients, otherwise replicon panics
    events.send(ServerEvent::ClientDisconnected { client_id, reason });
}

fn poll(
    mut replicon_server: ResMut<RepliconServer>,
    mut clients: Query<(Entity, &mut MessageBuffers, &Parent)>,
    open_servers: Query<(), OpenedServer>,
) {
    for (client, mut msg_bufs, server) in &mut clients {
        if open_servers.get(server.get()).is_err() {
            continue;
        }

        let client_id = client.into_client_id();
        for (lane_index, msg) in msg_bufs.recv.drain(..) {
            let Some(channel_id) = lane_index.try_into_channel_id() else {
                continue;
            };
            replicon_server.insert_received(client_id, channel_id, msg);
        }
    }
}

fn flush(mut replicon_server: ResMut<RepliconServer>, mut clients: Query<&mut MessageBuffers>) {
    for (client_id, channel_id, msg) in replicon_server.drain_sent() {
        let Some(mut msg_bufs) = client_id
            .try_into_entity()
            .and_then(|client| clients.get_mut(client).ok())
        else {
            continue;
        };
        let lane_index = channel_id.into_lane_index();
        msg_bufs.send.push(lane_index, msg);
    }
}
