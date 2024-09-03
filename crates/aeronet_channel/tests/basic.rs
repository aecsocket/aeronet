use aeronet::{
    server::ConnectedClients,
    transport::{DisconnectExt, SendMode, SendParams},
};
use aeronet_channel::{
    client::{send_to_server, ChannelClientPlugin},
    server::{ChannelServer, ChannelServerPlugin},
    transport::SpawnChannelClientExt,
};
use bevy::{log::LogPlugin, prelude::*};
use bevy_ecs::system::RunSystemOnce;

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

    app.world_mut().run_system_once_with(
        SendParams {
            client: local_client,
            mode: SendMode::ReliableUnordered,
            msg: b"hello world"[..].into(),
        },
        send_to_server.pipe(|In(msg_key)| info!("message key = {msg_key:?}")),
    );
    app.world_mut().run_system_once_with(
        SendParams {
            client: local_client,
            mode: SendMode::ReliableUnordered,
            msg: b"hello world #2"[..].into(),
        },
        send_to_server.pipe(|In(msg_key)| info!("message key = {msg_key:?}")),
    );

    // app.world_mut()
    //     .commands()
    //     .disconnect(local_client, "testing reason");
    // app.update();
}
