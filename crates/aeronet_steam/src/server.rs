use core::marker::PhantomData;

use aeronet_io::server::ServerEndpoint;
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

use crate::{SteamManager, session::SteamNetSessionPlugin};

pub struct SteamNetServerPlugin<M: SteamManager> {
    _phantom: PhantomData<M>,
}

impl<M: SteamManager> Default for SteamNetServerPlugin<M> {
    fn default() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }
}

impl<M: SteamManager> Plugin for SteamNetServerPlugin<M> {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<SteamNetSessionPlugin<M>>() {
            app.add_plugins(SteamNetSessionPlugin::<M>::default());
        }
    }
}

#[derive(Debug, Component)]
#[require(ServerEndpoint)]
pub struct SteamNetServer(());

impl SteamNetServer {
    #[must_use]
    pub fn connect() {}
}
