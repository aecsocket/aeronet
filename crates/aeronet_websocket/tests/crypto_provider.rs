#![expect(missing_docs, reason = "testing")]
#![cfg(not(target_family = "wasm"))]

use {
    aeronet_websocket::{
        client::{ClientConfig, WebSocketClientPlugin},
        server::{Identity, ServerConfig, WebSocketServerPlugin},
    },
    bevy::prelude::App,
};

#[test]
fn application_owns_crypto_provider_selection() {
    let mut app = App::new();
    app.add_plugins((WebSocketClientPlugin, WebSocketServerPlugin));
    assert!(rustls::crypto::CryptoProvider::get_default().is_none());

    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("application should install the first crypto provider");

    _ = ClientConfig::default();
    let identity = Identity::self_signed(["localhost"]).unwrap();
    _ = ServerConfig::builder()
        .with_bind_default(0)
        .with_identity(identity);
}
