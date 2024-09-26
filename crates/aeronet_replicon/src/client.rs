use {
    aeronet_io::{
        IoSet,
        connection::{Connected, Session},
    },
    aeronet_proto::{AeronetProtoPlugin, ProtoTransport, lane::LaneIndex, message::MessageBuffers},
    bevy_app::prelude::*,
    bevy_ecs::prelude::*,
    bevy_reflect::prelude::*,
    bevy_replicon::prelude::*,
    tracing::info,
};

#[derive(Debug)]
pub struct AeronetRepliconClientPlugin;

impl Plugin for AeronetRepliconClientPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<AeronetProtoPlugin>() {
            app.add_plugins(AeronetProtoPlugin);
        }

        app.register_type::<AeronetRepliconClient>()
            .configure_sets(
                PreUpdate,
                (IoSet::Poll, ClientIoSet::Poll, ClientSet::ReceivePackets).chain(),
            )
            .configure_sets(
                PostUpdate,
                (ClientSet::SendPackets, ClientIoSet::Flush, IoSet::Flush).chain(),
            )
            .add_systems(
                PreUpdate,
                (update_state, poll)
                    .chain()
                    .in_set(ClientIoSet::Poll)
                    .run_if(resource_exists::<RepliconClient>),
            )
            .add_systems(
                PostUpdate,
                flush
                    .in_set(ClientIoSet::Flush)
                    .run_if(resource_exists::<RepliconClient>),
            )
            .observe(on_client_added);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum ClientIoSet {
    Poll,
    Flush,
}

#[derive(Debug, Clone, Copy, Default, Component, Reflect)]
#[reflect(Component)]
pub struct AeronetRepliconClient;

// TODO: required components
fn on_client_added(trigger: Trigger<OnAdd, AeronetRepliconClient>, mut commands: Commands) {
    let client = trigger.entity();
    commands.entity(client).insert(ProtoTransport);
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
