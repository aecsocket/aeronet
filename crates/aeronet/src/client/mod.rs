use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_reflect::prelude::*;

use crate::transport::{DisconnectReason, TransportPlugin};

mod log;

#[derive(Debug)]
pub struct ClientTransportPlugin;

impl Plugin for ClientTransportPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<TransportPlugin>() {
            app.add_plugins(TransportPlugin);
        }

        app.register_type::<LocalClient>()
            .add_event::<LocalClientConnecting>()
            .add_event::<LocalClientConnected>()
            .add_event::<LocalClientDisconnected>()
            .add_plugins(log::EventLogPlugin);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Component, Reflect)]
#[reflect(Component)]
pub struct LocalClient;

#[derive(Debug, Clone, Event)]
pub struct LocalClientConnecting {
    pub client: Entity,
}

#[derive(Debug, Clone, Event)]
pub struct LocalClientConnected {
    pub client: Entity,
}

#[derive(Debug, Event)]
pub struct LocalClientDisconnected {
    pub client: Entity,
    pub reason: DisconnectReason,
}
