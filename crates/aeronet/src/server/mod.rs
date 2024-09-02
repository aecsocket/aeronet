use std::any::type_name;

use ahash::AHashSet;
use bevy_app::prelude::*;
use bevy_derive::Deref;
use bevy_ecs::{
    component::{ComponentHooks, StorageType},
    prelude::*,
};
use bevy_reflect::Reflect;
use bytes::Bytes;
use tracing::debug;

use crate::transport::{DisconnectReason, TransportPlugin};

mod log;

#[derive(Debug)]
pub struct ServerTransportPlugin;

impl Plugin for ServerTransportPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<TransportPlugin>() {
            app.add_plugins(TransportPlugin);
        }

        app.register_type::<Server>()
            .add_event::<ServerOpening>()
            .add_event::<ServerOpened>()
            .add_event::<ServerClosed>()
            .add_event::<RemoteClientConnecting>()
            .add_event::<RemoteClientConnected>()
            .add_event::<RemoteClientDisconnected>()
            .add_plugins(log::EventLogPlugin);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Component, Reflect)]
#[reflect(Component)]
pub struct Server;

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

#[derive(Debug, Clone, Default, Deref, Reflect)]
#[reflect(Component)]
pub struct ConnectedClients(#[reflect(ignore)] AHashSet<Entity>);

impl ConnectedClients {
    #[must_use]
    pub fn new() -> Self {
        Self(AHashSet::new())
    }

    #[must_use]
    pub fn with(self, added: Entity) -> Self {
        let mut clients = self.0;
        clients.insert(added);
        Self(clients)
    }

    #[must_use]
    pub fn without(self, removed: Entity) -> Self {
        let mut clients = self.0;
        clients.remove(&removed);
        Self(clients)
    }
}

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
    }
}

#[derive(Debug, Clone, Event)]
pub struct ServerOpening {
    pub server: Entity,
}

#[derive(Debug, Clone, Event)]
pub struct ServerOpened {
    pub server: Entity,
}

#[derive(Debug, Event)]
pub struct ServerClosed {
    pub server: Entity,
    pub reason: CloseReason,
}

#[derive(Debug)]
pub enum CloseReason {
    Local(String),
    Error(anyhow::Error),
}

#[derive(Debug, Clone, Event)]
pub struct RemoteClientConnecting {
    pub server: Entity,
    pub client: Entity,
}

#[derive(Debug, Clone, Event)]
pub struct RemoteClientConnected {
    pub server: Entity,
    pub client: Entity,
}

#[derive(Debug, Event)]
pub struct RemoteClientDisconnected {
    pub server: Entity,
    pub client: Entity,
    pub reason: DisconnectReason,
}

#[derive(Debug, Clone, Event)]
pub struct FromClient {
    pub server: Entity,
    pub client: Entity,
    pub msg: Bytes,
}
