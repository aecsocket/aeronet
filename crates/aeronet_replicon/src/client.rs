//! Client-side [`bevy_replicon`] support.

use {
    crate::convert,
    aeronet_io::{connection::Disconnect, web_time::Instant, Endpoint, Session},
    aeronet_transport::{AeronetTransportPlugin, Transport, TransportSet},
    bevy_app::prelude::*,
    bevy_ecs::prelude::*,
    bevy_reflect::prelude::*,
    bevy_replicon::prelude::*,
    tracing::warn,
};

/// Provides a [`bevy_replicon`] client backend using [`Session`]s for
/// communication.
///
/// To make a [`Session`] be used by [`bevy_replicon`], add the
/// [`AeronetRepliconClient`] component.
#[derive(Debug)]
pub struct AeronetRepliconClientPlugin;

impl Plugin for AeronetRepliconClientPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<AeronetTransportPlugin>() {
            app.add_plugins(AeronetTransportPlugin);
        }

        app.register_type::<AeronetRepliconClient>()
            .configure_sets(
                PreUpdate,
                (
                    TransportSet::Poll,
                    ClientTransportSet::Poll,
                    ClientSet::ReceivePackets,
                )
                    .chain(),
            )
            .configure_sets(
                PostUpdate,
                (
                    ClientSet::SendPackets,
                    ClientTransportSet::Flush,
                    TransportSet::Flush,
                )
                    .chain(),
            )
            .add_systems(
                PreUpdate,
                (update_state, poll)
                    .chain()
                    .in_set(ClientTransportSet::Poll)
                    .run_if(resource_exists::<RepliconClient>),
            )
            .add_systems(
                PostUpdate,
                flush
                    .in_set(ClientTransportSet::Flush)
                    .run_if(resource_exists::<RepliconClient>),
            )
            .observe(on_client_connected);
    }
}

/// Sets for systems which provide communication between [`bevy_replicon`] and
/// [`Session`]s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum ClientTransportSet {
    /// Passing incoming messages into [`bevy_replicon`].
    ///
    /// # Ordering
    ///
    /// - [`TransportSet::Poll`]
    /// - **[`ClientTransportSet::Poll`]**
    /// - [`ClientSet::ReceivePackets`]
    Poll,
    /// Passing outgoing [`bevy_replicon`] packets to the transport layer.
    ///
    /// # Ordering
    ///
    /// - [`ClientSet::SendPackets`]
    /// - **[`ClientTransportSet::Flush`]**
    /// - [`TransportSet::Flush`]
    Flush,
}

/// Marker component for a [`Session`] which is used as the messaging backend
/// for a [`RepliconClient`].
///
/// Sessions with this component automatically get [`Transport`].
///
/// Any session entity with this component will be used for:
/// - receiving messages
///   - on the `replicon` side, you can't differentiate which session received
///     which message
/// - sending messages
///   - all outgoing `replicon` messages are cloned and sent to all sessions
/// - determining connected status
///   - if at least 1 session is [`Connected`], [`RepliconClient`] is
///     [`RepliconClientStatus::Connected`]
///   - if at least 1 session exists, [`RepliconClient`] is
///     [`RepliconClientStatus::Connecting`]
///   - else, [`RepliconClientStatus::Disconnected`]
///
/// Since [`RepliconClient`] is a resource, there can only be up to one at a
/// time in the app, and you can only connect to one "logical" server at a time
/// (that is, the server which holds the actual app state). Therefore, your app
/// should only have one [`AeronetRepliconClient`].
#[derive(Debug, Clone, Copy, Default, Component, Reflect)]
#[reflect(Component)]
pub struct AeronetRepliconClient;

fn on_client_connected(
    trigger: Trigger<OnAdd, Session>,
    mut commands: Commands,
    clients: Query<&Session, With<AeronetRepliconClient>>,
    channels: Res<RepliconChannels>,
) {
    let client = trigger.entity();
    let Ok(session) = clients.get(client) else {
        return;
    };

    let recv_lanes = channels
        .server_channels()
        .iter()
        .map(|channel| convert::to_lane_kind(channel.kind));
    let send_lanes = channels
        .client_channels()
        .iter()
        .map(|channel| convert::to_lane_kind(channel.kind));
    let now = Instant::now();

    let transport = match Transport::new(&session, recv_lanes, send_lanes, now) {
        Ok(transport) => transport,
        Err(err) => {
            let err = anyhow::Error::new(err);
            warn!("Failed to create transport for {client}: {err:#}");
            commands.trigger_targets(Disconnect::new("failed to create transport"), client);
            return;
        }
    };

    commands.entity(client).insert(transport);
}

fn update_state(
    mut replicon_client: ResMut<RepliconClient>,
    clients: Query<Option<&Session>, (With<Endpoint>, With<AeronetRepliconClient>)>,
) {
    let status =
        clients.iter().fold(
            RepliconClientStatus::Disconnected,
            |status, session| match status {
                // if we've already found a connected client, then we are considered connected
                RepliconClientStatus::Connected { .. } => status,
                _ => {
                    // otherwise, we check if this client is connected..
                    if session.is_some() {
                        // ..and if so, then we're connected
                        RepliconClientStatus::Connected { client_id: None }
                    } else {
                        // ..otherwise, we know we are at least connecting
                        RepliconClientStatus::Connecting
                    }
                }
            },
        );

    if replicon_client.status() != status {
        replicon_client.set_status(status);
    }
}

fn poll(
    mut replicon_client: ResMut<RepliconClient>,
    mut clients: Query<&mut Transport, With<AeronetRepliconClient>>,
) {
    for mut transport in &mut clients {
        for msg in transport.recv_msgs.drain() {
            let Some(channel_id) = convert::to_channel_id(msg.lane) else {
                continue;
            };
            replicon_client.insert_received(channel_id, msg.payload);
        }

        for _ in transport.recv_acks.drain() {
            // we don't use the acks for anything
        }
    }
}

fn flush(
    mut replicon_client: ResMut<RepliconClient>,
    mut clients: Query<&mut Transport, With<AeronetRepliconClient>>,
) {
    let now = Instant::now();
    for (channel_id, msg) in replicon_client.drain_sent() {
        let lane_index = convert::to_lane_index(channel_id);
        for mut transport in &mut clients {
            transport.send.push(lane_index, msg.clone(), now);
        }
    }
}
