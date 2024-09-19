use {
    aeronet_webtransport::client::{
        ClientConfig, ConnectWebTransportClientExt, WebTransportClientPlugin,
    },
    bevy::{log::LogPlugin, prelude::*},
    std::time::{Duration, Instant},
};

#[test]
fn basic() {
    let mut app = App::new();
    app.add_plugins((
        LogPlugin {
            level: tracing::Level::DEBUG,
            ..default()
        },
        WebTransportClientPlugin,
    ));
    let session = app
        .world_mut()
        .commands()
        .connect_web_transport_client(ClientConfig::default(), "https://[::1]:25565");
    app.update();
    app.world_mut().despawn(session);

    let end_at = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end_at {
        app.update();
    }
}
