//! Implementation for Steam sessions.

use std::any::type_name;

use aeronet_io::connection::Connected;
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_steamworks::SteamworksPlugin;
use steamworks::{
    networking_sockets::NetConnection, networking_types::NetConnectionStatusChanged, ClientManager,
};

#[derive(Debug)]
pub(crate) struct SteamIoPlugin;

impl Plugin for SteamIoPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<SteamworksPlugin>() {
            panic!(
                "`{}` must be added before `{}`",
                type_name::<SteamworksPlugin>(),
                type_name::<Self>()
            );
        }

        let steam = app.world_mut().resource::<bevy_steamworks::Client>();

        app.observe(on_io_added);
    }
}

#[derive(Component)]
pub struct SteamIo {
    pub(crate) conn: NetConnection<ClientManager>,
}

// TODO: required components
fn on_io_added(trigger: Trigger<OnAdd, SteamIo>, mut commands: Commands) {
    let session = trigger.entity();
    commands.entity(session).insert(Connected);
}
