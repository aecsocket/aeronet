use {
    aeronet_webtransport::client::WebTransportClientPlugin,
    bevy::{log::LogPlugin, prelude::*},
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
}
