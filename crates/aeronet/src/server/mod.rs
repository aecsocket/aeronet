use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_reflect::prelude::*;
use bytes::Bytes;

use crate::transport::{DisconnectReason, TransportPlugin};

mod connection;
mod log;

pub use connection::*;

#[derive(Debug)]
pub struct ServerTransportPlugin;

impl Plugin for ServerTransportPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<TransportPlugin>() {
            app.add_plugins(TransportPlugin);
        }

        app.register_type::<Server>()
            .register_type::<Open>()
            .register_type::<connection::ConnectedClients>()
            .register_type::<connection::RemoteClient>()
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Component, Reflect)]
#[reflect(Component)]
pub struct Open;

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

#[derive(Debug, Clone, Event)]
pub struct RemoteClientConnecting {
    pub client: Entity,
}

#[derive(Debug, Clone, Event)]
pub struct RemoteClientConnected {
    pub client: Entity,
}

#[derive(Debug, Event)]
pub struct RemoteClientDisconnected {
    pub client: Entity,
    pub reason: DisconnectReason,
}

#[derive(Debug, Clone, Event)]
pub struct FromClient {
    pub client: Entity,
    pub msg: Bytes,
}

#[derive(Debug)]
pub enum CloseReason {
    Local(String),
    Error(anyhow::Error),
}
