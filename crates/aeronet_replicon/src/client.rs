//! Client-side [`bevy_replicon`] support.

use {
    crate::convert,
    aeronet_io::connection::{Connected, Session},
    aeronet_transport::{AeronetTransportPlugin, Transport, TransportSet, message::MessageBuffers},
    bevy_app::prelude::*,
    bevy_ecs::prelude::*,
    bevy_reflect::prelude::*,
    bevy_replicon::prelude::*,
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
            .observe(on_client_added);
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

// TODO: required components
fn on_client_added(
    trigger: Trigger<OnAdd, AeronetRepliconClient>,
    mut commands: Commands,
    channels: Res<RepliconChannels>,
) {
    let client = trigger.entity();

    let recv_lanes = channels
        .server_channels()
        .iter()
        .map(|channel| convert::to_lane_kind(channel.kind));
    let send_lanes = channels
        .client_channels()
        .iter()
        .map(|channel| convert::to_lane_kind(channel.kind));
    commands
        .entity(client)
        .insert(Transport::new(recv_lanes, send_lanes));
}

type ConnectedClient = (With<Session>, With<Connected>, With<AeronetRepliconClient>);

fn update_state(
    mut replicon_client: ResMut<RepliconClient>,
    clients: Query<Option<&Connected>, (With<Session>, With<AeronetRepliconClient>)>,
) {
    let status =
        clients.iter().fold(
            RepliconClientStatus::Disconnected,
            |status, connected| match status {
                // if we've already found a connected client, then we are considered connected
                RepliconClientStatus::Connected { .. } => status,
                _ => {
                    // otherwise, we check if this client is connected..
                    if connected.is_some() {
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
    mut clients: Query<&mut MessageBuffers, ConnectedClient>,
) {
    for mut msg_bufs in &mut clients {
        for (lane_index, msg) in msg_bufs.recv.drain(..) {
            let Some(channel_id) = convert::to_channel_id(lane_index) else {
                continue;
            };
            replicon_client.insert_received(channel_id, msg);
        }
    }
}

fn flush(
    mut replicon_client: ResMut<RepliconClient>,
    mut clients: Query<&mut MessageBuffers, ConnectedClient>,
) {
    for (channel_id, msg) in replicon_client.drain_sent() {
        let lane_index = convert::to_lane_index(channel_id);
        for mut msg_bufs in &mut clients {
            msg_bufs.send.push(lane_index, msg.clone());
        }
    }
}
