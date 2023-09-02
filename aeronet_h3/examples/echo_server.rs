use std::{net::SocketAddr, sync::Arc};

use aeronet::TransportSettings;
use aeronet_h3::{quinn, rustls, AsyncRuntime, H3ServerTransport};
use bevy::prelude::*;

pub struct AppTransportSettings;

impl TransportSettings for AppTransportSettings {
    type C2S = ();
    type S2C = ();
}

pub type ServerTransport = H3ServerTransport<AppTransportSettings>;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                fit_canvas_to_parent: true,
                prevent_default_event_handling: false,
                ..default()
            }),
            ..default()
        }))
        .init_resource::<AsyncRuntime>() // TODO this should be in a plugin
        .add_systems(Startup, setup)
        .run();
}

// TODO: this is *really* stupid. When you get rid of this, remove `macros` feature for tokio as well.
#[tokio::main]
async fn setup(mut commands: Commands, runtime: Res<AsyncRuntime>) {
    // the server.cert and server.key are copied from
    // https://github.com/hyperium/h3/tree/22da9387f19d724852b3bf1dfd7e66f0fd45cb81/examples
    let cert = rustls::Certificate(std::fs::read("./aeronet_h3/examples/server.cert").unwrap());
    let key = rustls::PrivateKey(std::fs::read("./aeronet_h3/examples/server.key").unwrap());

    let tls_config = rustls::ServerConfig::builder()
        .with_safe_default_cipher_suites()
        .with_safe_default_kx_groups()
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .unwrap();

    let config = quinn::ServerConfig::with_crypto(Arc::new(tls_config));
    let addr = "127.0.0.1:1995".parse::<SocketAddr>().unwrap();

    let transport = ServerTransport::new(config, addr).await.unwrap();

    commands.insert_resource(transport);
}
