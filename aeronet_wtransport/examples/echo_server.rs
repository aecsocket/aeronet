use std::{time::Duration, net::SocketAddr};

use aeronet_wtransport::{AsyncRuntime, server::plugin::WebTransportServer};
use anyhow::Result;
use bevy::{prelude::*, app::ScheduleRunnerPlugin};
use wtransport::{ServerConfig, tls::Certificate};

fn main() {
    App::new()
        .add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(100))))
        .init_resource::<AsyncRuntime>()
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands, rt: Res<AsyncRuntime>) {
    match create(rt.as_ref()) {
        Ok(server) => commands.insert_resource(server),
        Err(err) => error!("Failed to create server: {err:#}"),
    }
}

fn create(rt: &AsyncRuntime) -> Result<WebTransportServer> {
    let bind_addr = "[::1]:4433".parse::<SocketAddr>()?;
    let cert = Certificate::load("target/cert.pem", "target/key.pem")?;
    
    let config = ServerConfig::builder()
        .with_bind_address(bind_addr)
        .with_certificate(cert)
        .build();

    Ok(WebTransportServer::new(config, &rt.0))
}
