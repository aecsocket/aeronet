use std::time::Duration;

use aeronet::{
    AsyncRuntime, FromClient, RemoteClientDisconnected, ServerTransport, ServerTransportPlugin,
    TryFromBytes, TryIntoBytes,
};
use aeronet_wt_native::{
    wtransport::{tls::Certificate, ServerConfig},
    ServerStream, StreamMessage, TransportStreams, WebTransportServer,
};
use anyhow::Result;
use bevy::{
    app::{AppExit, ScheduleRunnerPlugin},
    log::LogPlugin,
    prelude::*,
};

// config

#[derive(Debug, Clone)]
pub struct AppMessage(pub String);

impl TryIntoBytes for AppMessage {
    fn try_into_bytes(self) -> Result<Vec<u8>> {
        Ok(self.0.into_bytes())
    }
}

impl TryFromBytes for AppMessage {
    fn try_from_bytes(payload: &[u8]) -> Result<Self> {
        String::from_utf8(payload.to_owned().into_iter().collect())
            .map(|s| AppMessage(s))
            .map_err(|err| err.into())
    }
}

type Server = WebTransportServer<AppMessage, StreamMessage<ServerStream, AppMessage>>;

// logic

/*
chromium \
--webtransport-developer-mode \
--ignore-certificate-errors-spki-list=x3S9HPqXZTYoR2tOQMmVG2GiZDPyyksnWdF9I9Ko/xY=
*/

fn main() {
    App::new()
        .add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(100))),
            LogPlugin {
                level: tracing::Level::DEBUG,
                ..default()
            },
            ServerTransportPlugin::<_, _, Server>::default(),
        ))
        .init_resource::<AsyncRuntime>()
        .add_systems(Startup, setup)
        .add_systems(Update, (reply, log_disconnect).chain())
        .run();
}

fn setup(mut commands: Commands, rt: Res<AsyncRuntime>) {
    let streams = TransportStreams::default();

    match create(rt.as_ref(), streams) {
        Ok(server) => {
            commands.insert_resource(server);
            info!("Created server");
        }
        Err(err) => error!("Failed to create server: {err:#}"),
    }
}

fn create(rt: &AsyncRuntime, streams: TransportStreams) -> Result<Server> {
    let cert = Certificate::load(
        "./aeronet_wt_native/examples/cert.pem",
        "./aeronet_wt_native/examples/key.pem",
    )?;

    let config = ServerConfig::builder()
        .with_bind_default(25565)
        .with_certificate(cert)
        .keep_alive_interval(Some(Duration::from_secs(5)))
        .build();

    let (front, back) = aeronet_wt_native::create_server(config, streams);
    rt.0.spawn(async move {
        back.start().await.unwrap();
    });
    Ok(front)
}

fn reply(
    mut server: ResMut<Server>,
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

fn log_disconnect(server: Res<Server>, mut dc: EventReader<RemoteClientDisconnected>) {
    for RemoteClientDisconnected { client, reason } in dc.iter() {
        info!(
            "Client {client} disconnected: {:#}",
            aeronet::error::as_pretty(reason)
        );
        info!("  Info: {:?}", server.client_info(*client));
    }
}
