use std::time::Duration;

use aeronet::{
    AsyncRuntime, FromClient, RecvMessage, RemoteClientDisconnected, ServerTransport,
    ServerTransportConfig, ServerTransportPlugin, TryIntoBytes,
};
use aeronet_wt_native::{
    wtransport::{tls::Certificate, ServerConfig},
    OnStream, ServerStream, StreamMessage, TransportStreams, WebTransportServer,
};
use anyhow::Result;
use bevy::{
    app::{AppExit, ScheduleRunnerPlugin},
    log::LogPlugin,
    prelude::*,
};

// config

pub struct AppTransportConfig;

impl ServerTransportConfig for AppTransportConfig {
    type C2S = AppMessage;
    type S2C = StreamMessage<ServerStream, AppMessage>;
}

#[derive(Debug, Clone)]
pub struct AppMessage(pub String);

impl TryIntoBytes for AppMessage {
    fn into_payload(self) -> Result<Vec<u8>> {
        Ok(self.0.into_bytes())
    }
}

impl RecvMessage for AppMessage {
    fn from_payload(payload: &[u8]) -> Result<Self> {
        String::from_utf8(payload.to_owned().into_iter().collect())
            .map(|s| AppMessage(s))
            .map_err(|err| err.into())
    }
}

// logic

// chromium --webtransport-developer-mode --ignore-certificate-errors-spki-list=x3S9HPqXZTYoR2tOQMmVG2GiZDPyyksnWdF9I9Ko/xY=

fn main() {
    App::new()
        .add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(100))),
            LogPlugin {
                level: tracing::Level::DEBUG,
                ..default()
            },
            ServerTransportPlugin::<AppTransportConfig, WebTransportServer<_>>::default(),
        ))
        .init_resource::<AsyncRuntime>()
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

fn create(rt: &AsyncRuntime) -> Result<WebTransportServer<AppTransportConfig>> {
    let cert = Certificate::load(
        "./aeronet_wt_native/examples/cert.pem",
        "./aeronet_wt_native/examples/key.pem",
    )?;

    let config = ServerConfig::builder()
        .with_bind_default(25565)
        .with_certificate(cert)
        .keep_alive_interval(Some(Duration::from_secs(5)))
        .build();

    let (front, back) = aeronet_wt_native::create_server(config, TransportStreams::default());
    rt.0.spawn(async move {
        back.start().await.unwrap();
    });
    Ok(front)
}

fn reply(
    mut server: ResMut<WebTransportServer<AppTransportConfig>>,
    mut recv: EventReader<FromClient<AppMessage>>,
    mut exit: EventWriter<AppExit>,
) {
    for FromClient { client, msg } in recv.iter() {
        info!("From {client}: {:?}", msg.0);
        info!("  Info: {:?}", server.client_info(*client));
        match msg.0.as_str() {
            "dc" => server.disconnect(*client),
            "stop" => exit.send(AppExit),
            msg => {
                let msg = format!("You sent: {}", msg);
                server.send(*client, AppMessage(msg).on(ServerStream::Datagram));
            }
        }
    }
}

fn log_disconnect(
    server: Res<WebTransportServer<AppTransportConfig>>,
    mut dc: EventReader<RemoteClientDisconnected>,
) {
    for RemoteClientDisconnected { client, reason } in dc.iter() {
        info!(
            "Client {client} disconnected: {:#}",
            aeronet::error::as_pretty(reason)
        );
        info!("  Info: {:?}", server.client_info(*client));
    }
}
