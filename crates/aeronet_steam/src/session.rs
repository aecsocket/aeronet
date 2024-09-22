//! Implementation for Steam sessions.

use std::any::type_name;

use aeronet_io::{connection::Connected, packet::PacketMtu};
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_steamworks::SteamworksPlugin;
use steamworks::{networking_sockets::NetConnection, ClientManager};

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

        app.observe(on_io_added);
    }
}

#[derive(Component)]
pub struct SteamIo {
    pub(crate) conn: NetConnection<ClientManager>,
}

// https://github.com/ValveSoftware/GameNetworkingSockets/blob/62b395172f157ca4f01eea3387d1131400f8d604/include/steam/steamnetworkingtypes.h#L837
const MTU: usize = 512 * 1024;

// TODO: required components
fn on_io_added(trigger: Trigger<OnAdd, SteamIo>, mut commands: Commands) {
    let session = trigger.entity();
    commands.entity(session).insert((Connected, PacketMtu(MTU)));
}
