use aeronet::{server::ConnectedClients, transport::DisconnectExt};
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
        LogPlugin {
            level: tracing::Level::TRACE,
            ..Default::default()
        },
        ChannelClientPlugin,
        ChannelServerPlugin,
    ));
    app.update();

    let mut commands = app.world_mut().commands();
    let server = commands
        .spawn((
            Name::new("Server"),
            ChannelServer,
            // TODO: required components
            // TODO: ConnectedClients MUST be spawned in the same archetype move as the server
            ConnectedClients::default(),
        ))
        .id();
    let (local_client, remote_client) = commands.spawn_channel_client(server);
    commands
        .entity(local_client)
        .insert(Name::new("Local client"));
    commands
        .entity(remote_client)
        .insert(Name::new("Remote client"));
    app.update();

    app.world_mut()
        .commands()
        .disconnect(local_client, "testing reason");
    app.update();
}
