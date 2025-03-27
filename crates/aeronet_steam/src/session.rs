//! Implementation for Steam sessions.

use std::any::type_name;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use steamworks::{ClientManager, networking_sockets::NetConnection};

#[derive(Debug)]
pub(crate) struct SteamIoPlugin;

impl Plugin for SteamIoPlugin {
    fn build(&self, app: &mut App) {
        let steam = app.world_mut().resource::<bevy_steamworks::Client>();

        app.observe(on_io_added);
    }
}

#[derive(Component)]
pub struct SteamIo {
    pub(crate) conn: NetConnection<ClientManager>,
}
