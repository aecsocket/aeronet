use std::{
    net::{Ipv6Addr, SocketAddr},
    time::Duration,
};

use aeronet_wtransport::{
    server::{WebTransportServer, WebTransportServerPlugin},
    AsyncRuntime, TransportConfig,
};
use anyhow::Result;
use bevy::{app::ScheduleRunnerPlugin, log::LogPlugin, prelude::*};
use wtransport::{tls::Certificate, ServerConfig};

pub struct AppTransportConfig;

impl TransportConfig for AppTransportConfig {
    type C2S = ();
    type S2C = ();
}

fn main() {
    App::new()
        .add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(100))),
            LogPlugin::default(),
            WebTransportServerPlugin::<AppTransportConfig>::default(),
        ))
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands, rt: Res<AsyncRuntime>) {
    match create(rt.as_ref()) {
        Ok(server) => {
            commands.insert_resource(server);
            info!("Created server");
        }
        Err(err) => error!("Failed to create server: {err:#}"),
    }
}

fn create(rt: &AsyncRuntime) -> Result<WebTransportServer<AppTransportConfig>> {
    let bind_addr = SocketAddr::new(Ipv6Addr::LOCALHOST.into(), 25565);
    let cert = Certificate::load(
        "./aeronet_wtransport/examples/cert.pem",
        "./aeronet_wtransport/examples/key.pem",
    )?;

    let config = ServerConfig::builder()
        .with_bind_address(bind_addr)
        .with_certificate(cert)
        .keep_alive_interval(Some(Duration::from_secs(5)))
        .build();

    Ok(WebTransportServer::new(config, &rt.0))
}
