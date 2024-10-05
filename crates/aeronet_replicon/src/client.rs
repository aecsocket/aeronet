//! Client-side [`bevy_replicon`] support.

use {
    aeronet_io::connection::{Connected, Session},
    aeronet_transport::{
        lane::LaneIndex, message::MessageBuffers, AeronetTransportPlugin, Transport, TransportSet,
    },
    bevy_app::prelude::*,
    bevy_ecs::prelude::*,
    bevy_reflect::prelude::*,
    bevy_replicon::prelude::*,
};

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

/// Set for scheduling systems in between the [`TransportSet`] and
/// [`bevy_replicon`]'s [`ClientSet`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum ClientTransportSet {
    /// Passing incoming messages into [`bevy_replicon`].
    Poll,
    /// Passing outgoing [`bevy_replicon`] packets to the transport layer.
    Flush,
}

/// Marker component for a client which uses a [`Session`] as the messaging
/// backend for a [`RepliconClient`].
///
/// Sessions with this component automatically get [`ProtoTransport`].
///
/// Any session entity with this component will be used for:
/// - receiving messages
/// - sending messages (all Replicon messages are sent to all sessions)
/// - determining connected status
///   - if at least 1 session is [`Connected`], [`RepliconClient`] is
///     [`RepliconClientStatus::Connected`]
///   - if at least 1 session exists, [`RepliconClient`] is
///     [`RepliconClientStatus::Connecting`]
///   - else, [`RepliconClientStatus::Disconnected`]
#[derive(Debug, Clone, Copy, Default, Component, Reflect)]
#[reflect(Component)]
pub struct AeronetRepliconClient;

// TODO: required components
fn on_client_added(trigger: Trigger<OnAdd, AeronetRepliconClient>, mut commands: Commands) {
    let client = trigger.entity();
    commands.entity(client).insert(Transport);
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
            let Ok(channel_id) = u8::try_from(lane_index.into_raw()) else {
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
        let lane_index = LaneIndex::from_raw(u64::from(channel_id));
        for mut msg_bufs in &mut clients {
            msg_bufs.send(lane_index, msg.clone());
        }
    }
}
