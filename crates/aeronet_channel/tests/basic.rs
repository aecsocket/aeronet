use aeronet::{
    server::{ConnectedClients, RemoteClient},
    transport::DisconnectExt,
};
use aeronet_channel::{
    client::ChannelClientPlugin,
    server::{ChannelServer, ChannelServerPlugin},
    transport::SpawnChannelClientExt,
};
use bevy::{log::LogPlugin, prelude::*};

#[test]
fn test() {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        LogPlugin::default(),
        ChannelClientPlugin,
        ChannelServerPlugin,
    ));
    app.update();

    let mut commands = app.world_mut().commands();
    let server = commands
        .spawn((ChannelServer, ConnectedClients::new()))
        .id();
    let (local_client, remote_client) = commands.spawn_channel_client(server);
    app.update();

    app.world_mut()
        .commands()
        .disconnect(remote_client, "testing reason");
    app.update();
}
