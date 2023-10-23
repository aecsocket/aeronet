use std::time::Duration;

use aeronet::{
    AsyncRuntime, DisconnectClient, FromClient, RemoteClientConnected, RemoteClientDisconnected,
    ServerTransport, ServerTransportPlugin, ToClient, TryFromBytes, TryIntoBytes,
};
use aeronet_wt_native::{Channels, OnChannel, WebTransportServer};
use anyhow::Result;
use bevy::{
    app::{AppExit, ScheduleRunnerPlugin},
    log::LogPlugin,
    prelude::*,
};
use wtransport::{tls::Certificate, ServerConfig};

// config

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Channels)]
#[channel_kind(Datagram)]
struct AppChannel;

#[derive(Debug, Clone, PartialEq, Eq, Hash, OnChannel)]
#[channel_type(AppChannel)]
#[on_channel(AppChannel)]
struct AppMessage(String);

impl TryFromBytes for AppMessage {
    fn try_from_bytes(buf: &[u8]) -> Result<Self> {
        String::from_utf8(buf.to_owned().into_iter().collect())
            .map(AppMessage)
            .map_err(Into::into)
    }
}

impl TryIntoBytes for AppMessage {
    fn try_into_bytes(self) -> Result<Vec<u8>> {
        Ok(self.0.into_bytes())
    }
}

type Server = WebTransportServer<AppMessage, AppMessage, AppChannel>;

// logic

/*
chromium \
--webtransport-developer-mode \
--ignore-certificate-errors-spki-list=x3S9HPqXZTYoR2tOQMmVG2GiZDPyyksnWdF9I9Ko/xY=
*/

fn main() {
    App::new()
        .add_plugins((
            LogPlugin {
                level: tracing::Level::DEBUG,
                ..default()
            },
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(100))),
            ServerTransportPlugin::<_, _, Server>::default(),
        ))
        .init_resource::<AsyncRuntime>()
        .add_systems(Startup, setup)
        .add_systems(Update, (reply, log))
        .run();
}

fn setup(mut commands: Commands, mut exit: EventWriter<AppExit>, rt: Res<AsyncRuntime>) {
    match create(rt.as_ref()) {
        Ok(server) => {
            info!("Created server");
            commands.insert_resource(server);
        }
        Err(err) => {
            error!("Failed to create server: {err:#}");
            exit.send(AppExit);
        }
    }
}

fn create(rt: &AsyncRuntime) -> Result<Server> {
    let cert = Certificate::load(
        "./aeronet_wt_native/examples/cert.pem",
        "./aeronet_wt_native/examples/key.pem",
    )?;

    let config = ServerConfig::builder()
        .with_bind_default(25565)
        .with_certificate(cert)
        .keep_alive_interval(Some(Duration::from_secs(5)))
        .build();

    let (front, back) = aeronet_wt_native::create_server(config);
    rt.0.spawn(async move {
        back.start().await.unwrap();
    });
    Ok(front)
}

fn log(
    server: Res<Server>,
    mut connected: EventReader<RemoteClientConnected>,
    mut disconnected: EventReader<RemoteClientDisconnected>,
) {
    for RemoteClientConnected(client) in connected.iter() {
        info!("Client {client} connected");
        info!("  Info: {:?}", server.client_info(*client));
    }

    for RemoteClientDisconnected(client, reason) in disconnected.iter() {
        info!(
            "Client {client} disconnected: {:#}",
            aeronet::error::as_pretty(reason),
        );
        info!("  Info: {:?}", server.client_info(*client));
    }
}

fn reply(
    mut recv: EventReader<FromClient<AppMessage>>,
    mut send: EventWriter<ToClient<AppMessage>>,
    mut disconnect: EventWriter<DisconnectClient>,
    mut exit: EventWriter<AppExit>,
) {
    for FromClient(client, msg) in recv.iter() {
        info!("From {client}: {:?}", msg.0);
        match msg.0.as_str() {
            "dc" => disconnect.send(DisconnectClient(*client)),
            "stop" => exit.send(AppExit),
            msg => {
                let msg = format!("You sent: {}", msg);
                send.send(ToClient(*client, AppMessage(msg)));
            }
        }
    }
}
