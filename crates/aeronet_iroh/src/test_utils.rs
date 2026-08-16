//! Utilities for testing apps using `aeronet_iroh`.
//!
//! Enable the `test-utils` feature to use this module.

use {
    iroh::{RelayMap, RelayUrl},
    iroh_relay::server::Server,
};

/// Starts an Iroh relay server bound to loopback, suitable for tests.
///
/// Returns the [`RelayMap`] to configure endpoints with
/// (`iroh::RelayMode::Custom`), the [`RelayUrl`] to use as the dial-side
/// addressing hint, and the running server. The relay speaks QUIC as well as
/// HTTPS, so hole punching between test endpoints works the same way it does
/// against a production relay.
///
/// The server shuts down when the returned [`Server`] is dropped.
///
/// No external network access is required.
///
/// # Panics
///
/// Panics if the relay server fails to bind.
pub async fn run_test_relay() -> (RelayMap, RelayUrl, Server) {
    use {
        core::net::Ipv4Addr,
        iroh_relay::{
            RelayConfig, RelayQuicConfig,
            server::{
                CertConfig, QuicConfig, RelayConfig as RelayServerConfig, ServerConfig, TlsConfig,
            },
        },
    };

    let (_certs, server_config) = iroh_relay::server::testing::self_signed_tls_certs_and_config();
    let tls = TlsConfig::new(
        (Ipv4Addr::LOCALHOST, 0),
        CertConfig::Manual { server_config },
    );

    let mut relay = RelayServerConfig::new((Ipv4Addr::LOCALHOST, 0));
    relay.tls = Some(tls);

    let mut config = ServerConfig::default();
    config.relay = Some(relay);
    config.quic = Some(QuicConfig::new((Ipv4Addr::LOCALHOST, 0)));

    let server = Server::spawn(config)
        .await
        .expect("failed to spawn test relay server");
    let url = server.https_url().expect("TLS was configured");
    let quic = server
        .quic_addr()
        .map(|addr| RelayQuicConfig::new(addr.port()));
    let map = RelayMap::from(RelayConfig::new(url.clone(), quic));
    (map, url, server)
}
