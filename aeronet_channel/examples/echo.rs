use std::time::Duration;

use aeronet_channel::ChannelServerTransport;
use bevy::{app::ScheduleRunnerPlugin, prelude::*};

fn main() {
    App::new()
        .add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(100))))
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands) {
    let mut server_tx = ChannelServerTransport::new();
    let client_tx = server_tx.connect();

    commands.insert_resource(server_tx);
    commands.insert_resource(client_tx);
}
