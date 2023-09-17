use std::time::Duration;

use aeronet::{AsyncRuntime, Message, TransportConfig};
use aeronet_wtransport::{
    server::{
        plugin::{ServerClientDisconnected, ServerRecv, WtServerPlugin},
        Frontend, ServerStream,
    },
    Streams,
};
use anyhow::Result;
use bevy::{
    app::ScheduleRunnerPlugin,
    log::{Level, LogPlugin},
    prelude::*,
};
use wtransport::{tls::Certificate, ServerConfig};

pub struct AppTransportConfig;

impl TransportConfig for AppTransportConfig {
    type C2S = AppMessage;
    type S2C = AppMessage;
}

#[derive(Debug, Clone)]
pub struct AppMessage(pub String);

impl Message for AppMessage {
    fn from_payload(payload: &[u8]) -> Result<Self> {
        String::from_utf8(payload.to_owned().into_iter().collect())
            .map(|s| AppMessage(s))
            .map_err(|err| anyhow::Error::new(err))
    }

    fn into_payload(self) -> Result<Vec<u8>> {
        Ok(self.0.into_bytes())
    }
}

// chromium --origin-to-force-quic-on=localhost:25565 --ignore-certificate-errors-spki-list=x3S9HPqXZTYoR2tOQMmVG2GiZDPyyksnWdF9I9Ko/xY=

fn main() {
    App::new()
        .add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(100))),
            WtServerPlugin::<AppTransportConfig>::default(),
            LogPlugin {
                level: Level::DEBUG,
                ..default()
            },
        ))
        .add_systems(Startup, setup)
        .add_systems(Update, (reply, log_disconnect).chain())
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

fn create(rt: &AsyncRuntime) -> Result<Frontend<AppTransportConfig>> {
    let cert = Certificate::load(
        "./aeronet_wtransport/examples/cert.pem",
        "./aeronet_wtransport/examples/key.pem",
    )?;

    let config = ServerConfig::builder()
        .with_bind_default(25565)
        .with_certificate(cert)
        .keep_alive_interval(Some(Duration::from_secs(5)))
        .build();

    let mut streams = Streams::new();
    //streams.add_bi();
    //streams.add_c2s();

    let (front, back) = aeronet_wtransport::server::create::<AppTransportConfig>(config, streams);
    rt.0.spawn(async move {
        back.listen().await.unwrap();
    });
    Ok(front)
}

fn reply(server: Res<Frontend<AppTransportConfig>>, mut recv: EventReader<ServerRecv<AppMessage>>) {
    for ServerRecv { client, msg } in recv.iter() {
        info!("From {client}: {:?}", msg.0);
        info!("  {:?}", server.client_info(*client));
        match msg.0.as_str() {
            "dc" => server.disconnect(*client),
            _ => server.send(
                *client,
                ServerStream::Datagram,
                AppMessage("Acknowledged".into()),
            ),
        }
    }
}

fn log_disconnect(
    server: Res<Frontend<AppTransportConfig>>,
    mut dc: EventReader<ServerClientDisconnected>,
) {
    for ServerClientDisconnected { client, reason } in dc.iter() {
        info!(
            "Client {client} disconnected: {:#}",
            aeronet::error::as_pretty(reason)
        );
        info!("  {:?}", server.client_info(*client));
    }
}
