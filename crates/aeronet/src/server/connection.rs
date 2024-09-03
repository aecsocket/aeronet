use std::any::type_name;

use ahash::AHashSet;
use bevy_derive::Deref;
use bevy_ecs::{
    component::{ComponentHooks, StorageType},
    prelude::*,
};
use bevy_reflect::prelude::*;
use tracing::debug;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deref, Reflect)]
#[reflect(Component)]
pub struct RemoteClient(Entity);

impl RemoteClient {
    #[must_use]
    pub const fn new(server: Entity) -> Self {
        Self(server)
    }

    #[must_use]
    pub const fn server(self) -> Entity {
        self.0
    }
}

impl Component for RemoteClient {
    const STORAGE_TYPE: StorageType = StorageType::Table;

    fn register_component_hooks(hooks: &mut ComponentHooks) {
        hooks.on_insert(|mut world, client, _| {
            let &RemoteClient(server) = world
                .get::<RemoteClient>(client)
                .expect("we are inserting this component");

            let mut server_clients =
                world
                    .get_mut::<ConnectedClients>(server)
                    .unwrap_or_else(|| {
                        panic!(
                            "inserted `{}` into client {client:?} pointing to server {server:?}, \
                            but server doesn't have `{}`",
                            type_name::<RemoteClient>(),
                            type_name::<ConnectedClients>(),
                        );
                    });

            if server_clients.0.insert(client) {
                debug!("Inserted client {client:?} as connected client of server {server:?}");
            }
        });

        hooks.on_remove(|mut world, client, _| {
            let &RemoteClient(server) = world
                .get::<RemoteClient>(client)
                .expect("we are removing this component");

            let mut server_clients =
                world
                    .get_mut::<ConnectedClients>(server)
                    .unwrap_or_else(|| {
                        panic!(
                            "removed `{}` from client {client:?} pointing to server {server:?}, \
                            but server doesn't have `{}`",
                            type_name::<RemoteClient>(),
                            type_name::<ConnectedClients>(),
                        );
                    });

            if server_clients.0.remove(&client) {
                debug!("Removed client {client:?} as connected client of server {server:?}");
            } else {
                panic!(
                    "removed `{}` from client {client:?} pointing to server {server:?}, \
                    but server doesn't have this client in its `{}`",
                    type_name::<RemoteClient>(),
                    type_name::<ConnectedClients>(),
                );
            }
        });
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deref, Reflect)]
#[reflect(Component)]
pub struct ConnectedClients(#[reflect(ignore)] AHashSet<Entity>);

impl Component for ConnectedClients {
    const STORAGE_TYPE: StorageType = StorageType::Table;

    fn register_component_hooks(hooks: &mut ComponentHooks) {
        hooks.on_insert(|world, server, _| {
            let ConnectedClients(clients) = world
                .get::<ConnectedClients>(server)
                .expect("we are inserting this component");

            for &client in clients {
                let &RemoteClient(connected_server) = world.get::<RemoteClient>(client).unwrap_or_else(|| {
                    panic!(
                        "inserted `{}` into server {server:?}, \
                        but connected client {client:?} doesn't have `{}`",
                        type_name::<ConnectedClients>(),
                        type_name::<RemoteClient>(),
                    );
                });

                if connected_server != server {
                    panic!(
                        "inserted `{}` into server {server:?}, \
                        but connected client {client:?} is pointing at {connected_server:?}, not this server - \
                        make sure you are inserting `{}` first",
                        type_name::<ConnectedClients>(),
                        type_name::<RemoteClient>(),
                    );
                }
            }
        });

        hooks.on_remove(|world, server, _| {
            let ConnectedClients(clients) = world
                .get::<ConnectedClients>(server)
                .expect("we are inserting this component");

            if !clients.is_empty() {
                panic!(
                    "removed `{}` from server {server:?}, but it still has connected clients - \
                    all connected clients must have their `{}`s removed first",
                    type_name::<ConnectedClients>(),
                    type_name::<RemoteClient>(),
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_remote_clients() {
        let mut world = World::new();
        let server = world.spawn(ConnectedClients::default()).id();

        assert_eq!(
            AHashSet::new(),
            world.get::<ConnectedClients>(server).unwrap().0
        );

        let client1 = world.spawn(RemoteClient::new(server)).id();
        assert_eq!(
            AHashSet::from([client1]),
            world.get::<ConnectedClients>(server).unwrap().0
        );

        let client2 = world.spawn(RemoteClient::new(server)).id();
        assert_eq!(
            AHashSet::from([client1, client2]),
            world.get::<ConnectedClients>(server).unwrap().0
        );
    }

    #[test]
    fn remove_remote_clients() {
        let mut world = World::new();
        let server = world.spawn(ConnectedClients::default()).id();
        let client1 = world.spawn(RemoteClient::new(server)).id();
        let client2 = world.spawn(RemoteClient::new(server)).id();

        assert_eq!(
            AHashSet::from([client1, client2]),
            world.get::<ConnectedClients>(server).unwrap().0
        );

        world.entity_mut(client1).remove::<RemoteClient>();
        assert_eq!(
            AHashSet::from([client2]),
            world.get::<ConnectedClients>(server).unwrap().0
        );

        world.entity_mut(client2).remove::<RemoteClient>();
        assert_eq!(
            AHashSet::new(),
            world.get::<ConnectedClients>(server).unwrap().0
        );
    }

    #[test]
    #[should_panic]
    fn remove_connected_clients_with_oustanding_remote_clients() {
        let mut world = World::new();
        let server = world.spawn(ConnectedClients::default()).id();
        world.spawn(RemoteClient::new(server));
        world.entity_mut(server).remove::<ConnectedClients>();
    }

    #[test]
    #[should_panic]
    fn add_connected_client_without_remote_client() {
        let mut world = World::new();
        let client1 = world.spawn_empty().id();
        world.spawn(ConnectedClients(AHashSet::from([client1])));
    }

    #[test]
    fn despawn_entity_with_no_connected_clients() {
        let mut world = World::new();
        let server = world.spawn(ConnectedClients::default()).id();
        world.despawn(server);
    }

    #[test]
    #[should_panic]
    fn despawn_entity_with_connected_clients() {
        let mut world = World::new();
        let server = world.spawn(ConnectedClients::default()).id();
        world.spawn(RemoteClient::new(server));
        world.despawn(server);
    }
}
