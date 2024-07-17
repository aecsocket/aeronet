#![doc = include_str!("../README.md")]

use aeronet_replicon::{client::RepliconClientPlugin, server::RepliconServerPlugin};
use aeronet_webtransport::{
    client::WebTransportClient,
    server::{ClientKey, WebTransportServer},
};
use bevy::{
    prelude::*,
    winit::{UpdateMode, WinitSettings},
};
use bevy_replicon::{
    prelude::{AppRuleExt, ChannelKind, ClientEventAppExt},
    RepliconPlugins,
};
use serde::{Deserialize, Serialize};

/// `move_box` demo server
#[derive(Debug, clap::Parser)]
enum Args {
    /// Runs the server side
    Server {
        /// Port to listen on
        #[arg(long, default_value_t = DEFAULT_PORT)]
        port: u16,
    },
    /// Runs the client side
    Client {
        /// URL of the server to connect to
        target: String,
    },
}

const DEFAULT_PORT: u16 = 25565;

fn main() {
    App::new()
        // setup
        .add_plugins((
            DefaultPlugins,
            RepliconPlugins,
            RepliconClientPlugin::<WebTransportClient>::default(),
            RepliconServerPlugin::<WebTransportServer>::default(),
        ))
        .init_resource::<WebTransportClient>()
        .init_resource::<WebTransportServer>()
        // update the app even when unfocused
        .insert_resource(WinitSettings {
            focused_mode: UpdateMode::Continuous,
            unfocused_mode: UpdateMode::Continuous,
        })
        // game logic
        .replicate::<Player>()
        .replicate::<PlayerPosition>()
        .replicate::<PlayerColor>()
        .add_client_event::<MoveDirection>(ChannelKind::Ordered)
        .add_systems(Update, update_inputs);
}
