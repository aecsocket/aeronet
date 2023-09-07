use std::{net::SocketAddr, time::Duration};

use aeronet_wtransport::AsyncRuntime;
use anyhow::Result;
use bevy::prelude::*;
use tokio::sync::mpsc;
use wtransport::{ClientConfig, Endpoint};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .init_resource::<AsyncRuntime>()
        .add_systems(Startup, setup)
        .run();
}

async fn send_err<E: std::error::Error>(errors: &mut mpsc::Sender<E>, err: E) {
    let Err(err) = errors.send(err).await else { return; };
    let err = err.0;
    warn!("Failed to send error on disconnected channel: {err:#}");
}

async fn send_on_err<E: std::error::Error>(
    errors: &mut mpsc::Sender<E>,
    block: impl FnOnce() -> Result<(), E>,
) {
    let Err(err) = block() else { return; };
    send_err(errors, err).await;
}

fn setup(rt: Res<AsyncRuntime>) {
    let bind_addr = "[::1]:0"
        .parse::<SocketAddr>()
        .expect("parsing address should not fail");

    let config = ClientConfig::builder()
        .with_bind_address(bind_addr)
        .with_native_certs()
        .keep_alive_interval(Some(Duration::from_secs(1)))
        .max_idle_timeout(Some(Duration::from_secs(30)))
        .expect("timeout is valid")
        .build();

    rt.0.spawn(async move {});
}

async fn create_endpoint(config: ClientConfig) -> Result<()> {
    let endpoint = Endpoint::client(config)?;
    let conn = endpoint.connect("https://[::1]:4433").await?;

    info!("Connected");

    let (send, mut recv) = conn.open_bi().await?.await?;

    let mut buf = [0u8; 1024];
    recv.read(&mut buf).await?;

    info!("Got: {:?}", buf);

    Ok(())
}
