use {
    aeronet_io::{
        connection::Connected,
        server::{Opened, Server},
        IoSet,
    },
    aeronet_proto::{lane::LaneIndex, message::MessageBuffers},
    ahash::AHashMap,
    bevy_app::prelude::*,
    bevy_derive::Deref,
    bevy_ecs::{
        component::{ComponentHooks, StorageType},
        prelude::*,
    },
    bevy_hierarchy::Parent,
    bevy_reflect::Reflect,
    bevy_replicon::{core::ClientId, prelude::RepliconServer, server::ServerSet},
    std::collections::hash_map::Entry,
};

#[derive(Debug)]
pub struct AeronetRepliconServerPlugin;

impl Plugin for AeronetRepliconServerPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<AeronetRepliconServer>()
            .register_type::<RepliconId>()
            .init_resource::<ClientIdMap>()
            .configure_sets(
                PreUpdate,
                (IoSet::Poll, ServerIoSet::Poll, ServerSet::ReceivePackets).chain(),
            )
            .configure_sets(
                PostUpdate,
                (ServerSet::SendPackets, ServerIoSet::Flush, IoSet::Flush).chain(),
            )
            .add_systems(
                PreUpdate,
                (poll, update_state)
                    .chain()
                    .in_set(ServerIoSet::Poll)
                    .run_if(resource_exists::<RepliconServer>),
            )
            .add_systems(
                PostUpdate,
                flush
                    .in_set(ServerIoSet::Flush)
                    .run_if(resource_exists::<RepliconServer>),
            );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum ServerIoSet {
    Poll,
    Flush,
}

#[derive(Debug, Clone, Copy, Default, Component, Reflect)]
#[reflect(Component)]
pub struct AeronetRepliconServer;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deref, Reflect)]
#[reflect(Component)]
pub struct RepliconId(ClientId);

impl RepliconId {
    #[must_use]
    pub const fn get(self) -> ClientId {
        self.0
    }
}

impl Component for RepliconId {
    const STORAGE_TYPE: StorageType = StorageType::Table;

    fn register_component_hooks(hooks: &mut ComponentHooks) {
        hooks.on_insert(|mut world, entity, _| {
            let &RepliconId(client_id) = world
                .get::<RepliconId>(entity)
                .expect("component should be present after insertion");
            let mut client_id_map = world.resource_mut::<ClientIdMap>();
            match client_id_map.0.entry(client_id) {
                Entry::Occupied(entry) => {
                    let already_used_by = entry.get();
                    panic!(
                        "attempted to insert {client_id:?} into {entity}, \
                        but this ID is already used by {already_used_by}"
                    );
                }
                Entry::Vacant(entry) => {
                    entry.insert(entity);
                }
            }
        });

        hooks.on_remove(|mut world, entity, _| {
            let &RepliconId(client_id) = world
                .get::<RepliconId>(entity)
                .expect("component should be present before removal");
            let mut client_id_map = world.resource_mut::<ClientIdMap>();

            let Some(previous_entity) = client_id_map.0.remove(&client_id) else {
                panic!(
                    "attempted to remove {client_id:?} from {entity}, \
                    but this ID was not mapped to any entity"
                );
            };

            if previous_entity != entity {
                panic!(
                    "attempted to remove {client_id:?} from {entity}, \
                    but this ID was mapped to {previous_entity}"
                );
            }
        });
    }
}

#[derive(Debug, Clone, Default, Deref, Resource, Reflect)]
#[reflect(Resource)]
pub struct ClientIdMap(#[reflect(ignore)] AHashMap<ClientId, Entity>);

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

fn poll(
    mut replicon_server: ResMut<RepliconServer>,
    mut clients: Query<(&mut MessageBuffers, &RepliconId, &Parent)>,
    open_servers: Query<(), OpenedServer>,
) {
    for (mut msg_bufs, &RepliconId(client_id), server) in &mut clients {
        if open_servers.get(server.get()).is_err() {
            continue;
        }

        for (lane_index, msg) in msg_bufs.recv.drain(..) {
            let Ok(channel_id) = u8::try_from(lane_index.into_raw()) else {
                continue;
            };
            replicon_server.insert_received(client_id, channel_id, msg);
        }
    }
}

fn flush(
    mut replicon_server: ResMut<RepliconServer>,
    client_id_map: Res<ClientIdMap>,
    mut clients: Query<&mut MessageBuffers, Without<Parent>>,
) {
    for (client_id, channel_id, msg) in replicon_server.drain_sent() {
        let Some(&client) = client_id_map.get(&client_id) else {
            continue;
        };
        let Ok(mut msg_bufs) = clients.get_mut(client) else {
            continue;
        };
        let lane_index = LaneIndex::from_raw(u64::from(channel_id));
        msg_bufs.send(lane_index, msg);
    }
}
