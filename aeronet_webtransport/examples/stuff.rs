use std::time::Duration;

use aeronet::{
    protocol::{ProtocolVersion, TransportProtocol},
    server::ServerTransport,
};
use aeronet_replicon::protocol::RepliconMessage;
use aeronet_webtransport::{
    server::{ServerConfig, WebTransportServer},
    shared::WebTransportProtocol,
};

#[derive(Debug, Clone, Copy, TransportProtocol)]
#[c2s(RepliconMessage)]
#[s2c(RepliconMessage)]
struct AppProtocol;

impl WebTransportProtocol for AppProtocol {
    type Mapper = ();
}

const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion(0xbabad0d0bebebaba);

#[tokio::main]
async fn main() {
    let cert = wtransport::Certificate::load(
        "aeronet_webtransport/examples/cert.pem",
        "aeronet_webtransport/examples/key.pem",
    )
    .await
    .unwrap();
    let native_config = wtransport::ServerConfig::builder()
        .with_bind_default(25565)
        .with_certificate(cert)
        .keep_alive_interval(Some(Duration::from_secs(5)))
        .build();
    let config = ServerConfig::new(native_config, ());
    let (mut server, backend) = WebTransportServer::<AppProtocol>::open_new(config);
    tokio::spawn(backend);
    loop {
        let _ = server.poll(Duration::from_millis(10)).count();
    }
}
