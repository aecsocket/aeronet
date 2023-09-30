use std::time::Duration;

use aeronet::{
    AsyncRuntime, ClientDisconnected, FromClient, RecvMessage, SendMessage, ServerTransport,
    ServerTransportConfig,
};
use aeronet_wt_native::{ServerMessage, ServerStream, StreamDefinitions, WebTransportServer, OnServerStream};
use anyhow::Result;
use bevy::{
    app::ScheduleRunnerPlugin,
    log::{Level, LogPlugin},
    prelude::*,
};
use wtransport::{tls::Certificate, ServerConfig};

pub struct AppTransportConfig;

impl ServerTransportConfig for AppTransportConfig {
    type C2S = AppMessage;
    type S2C = ServerMessage<AppMessage>;
}

#[derive(Debug, Clone)]
pub struct AppMessage(pub String);

impl SendMessage for AppMessage {
    fn into_payload(self) -> Result<Vec<u8>> {
        Ok(self.0.into_bytes())
    }
}

impl RecvMessage for AppMessage {
    fn from_payload(payload: &[u8]) -> Result<Self> {
        String::from_utf8(payload.to_owned().into_iter().collect())
            .map(|s| AppMessage(s))
            .map_err(|err| anyhow::Error::new(err))
    }
}

// chromium --webtransport-developer-mode --ignore-certificate-errors-spki-list=x3S9HPqXZTYoR2tOQMmVG2GiZDPyyksnWdF9I9Ko/xY=

fn main() {
    App::new()
        .add_plugins(aeronet::ServerTransportPlugin::<
            AppTransportConfig,
            aeronet_wt_native::WebTransportServer<_>,
        >::default())
        .add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(100))),
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

fn create(rt: &AsyncRuntime) -> Result<WebTransportServer<AppTransportConfig>> {
    let cert = Certificate::load(
        "./aeronet_webtransport/examples/cert.pem",
        "./aeronet_webtransport/examples/key.pem",
    )?;

    let config = ServerConfig::builder()
        .with_bind_default(25565)
        .with_certificate(cert)
        .keep_alive_interval(Some(Duration::from_secs(5)))
        .build();

    let mut streams = StreamDefinitions::new();
    //streams.add_bi();
    //streams.add_c2s();

    let (front, back) = aeronet_wt_native::create_server(config, streams);
    rt.0.spawn(async move {
        back.listen().await.unwrap();
    });
    Ok(front)
}

fn reply(
    mut server: ResMut<WebTransportServer<AppTransportConfig>>,
    mut recv: EventReader<FromClient<AppMessage>>,
) {
    for FromClient { client, msg } in recv.iter() {
        info!("From {client}: {:?}", msg.0);
        info!("  {:?}", server.client_info(*client));
        match msg.0.as_str() {
            "dc" => server.disconnect(*client),
            _ => server.send(
                *client,
                AppMessage("Acknowledged".into()).on(ServerStream::Datagram),
            ),
        }
    }
}

fn log_disconnect(
    server: Res<WebTransportServer<AppTransportConfig>>,
    mut dc: EventReader<ClientDisconnected>,
) {
    for ClientDisconnected { client, reason } in dc.iter() {
        info!(
            "Client {client} disconnected: {:#}",
            aeronet::error::as_pretty(reason)
        );
        info!("  {:?}", server.client_info(*client));
    }
}
