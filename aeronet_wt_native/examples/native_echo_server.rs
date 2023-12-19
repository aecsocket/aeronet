//!

use std::time::Duration;

use aeronet::{AsyncRuntime, ClientState, TransportServer};
use aeronet_example::EchoProtocol;
use aeronet_wt_native::{ServerEvent, WebTransportServer};
use anyhow::Result;
use bevy::{app::ScheduleRunnerPlugin, log::LogPlugin, prelude::*};
use wtransport::{tls::Certificate, ServerConfig};

type Server = WebTransportServer<EchoProtocol>;

// logic

/*
chromium \
brave \
--webtransport-developer-mode \
--ignore-certificate-errors-spki-list=x3S9HPqXZTYoR2tOQMmVG2GiZDPyyksnWdF9I9Ko/xY=
*/

fn main() {
    App::new()
        .add_plugins((
            LogPlugin {
                filter: "aeronet_wt_native=debug".to_string(),
                ..default()
            },
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(100))),
        ))
        .init_resource::<AsyncRuntime>()
        .add_systems(Startup, setup)
        .add_systems(Update, update_server)
        .run();
}

fn setup(mut commands: Commands, rt: Res<AsyncRuntime>) {
    match create(rt.as_ref()) {
        Ok(server) => {
            info!("Created server");
            commands.insert_resource(server);
        }
        Err(err) => panic!("Failed to create server: {err:#}"),
    }
}

fn create(rt: &AsyncRuntime) -> Result<Server> {
    let cert = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(Certificate::load(
            "./aeronet_wt_native/examples/cert.pem",
            "./aeronet_wt_native/examples/key.pem",
        ))?;

    let config = ServerConfig::builder()
        .with_bind_default(25565)
        .with_certificate(cert)
        .keep_alive_interval(Some(Duration::from_secs(5)))
        .build();

    let (server, backend) = WebTransportServer::opening(config);
    rt.0.spawn(backend);

    Ok(WebTransportServer::from(server))
}

fn update_server(mut server: ResMut<Server>) {
    for event in server.recv() {
        match event {
            ServerEvent::Opened => info!("Opened server for connections"),
            ServerEvent::Incoming { client } => info!("{client:?} incoming"),
            ServerEvent::Accepted {
                client,
                authority,
                path,
                ..
            } => info!("{client:?} accepted from {authority}{path}"),
            ServerEvent::Connected { client } => {
                let ClientState::Connected(info) = server.client_state(client) else {
                    unreachable!();
                };
                info!("{client:?} connected from {}", info.remote_addr);
                let _ = server.send(client, "Welcome!");
                let _ = server.send(client, "Send me some text, and I will send it back!");
            }
            ServerEvent::Recv { client, msg } => {
                info!("{client:?} > {}", msg.0);
                let msg = format!("You sent: {}", msg.0);
                let _ = server.send(client, msg);
            }
            ServerEvent::Disconnected { client, cause } => info!(
                "{client:?} disconnected: {:#}",
                aeronet::error::as_pretty(&cause)
            ),
            ServerEvent::Closed { cause } => {
                info!("Server closed: {:#}", aeronet::error::as_pretty(&cause))
            }
        }
    }
}
