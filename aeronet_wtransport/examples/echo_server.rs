use std::{time::Duration, net::SocketAddr};

use aeronet_wtransport::{AsyncRuntime, WebTransportServer};
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
    rt.0.spawn(async move {
        match create().await {
            Ok(server) => commands.insert_resource(server),
            Err(err) => error!("Failed to create server: {err:#}"),
        }
    });
}

async fn create() -> Result<WebTransportServer> {
    let bind_addr = "[::1]:4433".parse::<SocketAddr>()?;

    let cert = std::fs::read("./aeronet_wtransport/examples/server.cert")?;
    let private_key = std::fs::read("./aeronet_wtransport/examples/server.key")?;

    let config = ServerConfig::builder()
        .with_bind_address(bind_addr)
        .with_certificate(Certificate::new(vec![cert], private_key))
        .build();

    let server = WebTransportServer::new(config).await?;
    Ok(server)
}
