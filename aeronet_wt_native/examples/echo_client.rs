use std::time::Duration;

use aeronet::{
    AsyncRuntime, ClientTransportConfig, ClientTransportPlugin, LocalClientConnected,
    LocalClientDisconnected, RecvMessage, TryIntoBytes,
};
use aeronet_wt_native::{
    wtransport::ClientConfig, ClientStream, StreamMessage, TransportStreams, WebTransportClient,
};
use anyhow::Result;
use bevy::{log::LogPlugin, prelude::*};

// config

pub struct AppTransportConfig;

impl ClientTransportConfig for AppTransportConfig {
    type C2S = StreamMessage<ClientStream, AppMessage>;
    type S2C = AppMessage;
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

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins.set(LogPlugin {
                level: tracing::Level::DEBUG,
                ..default()
            }),
            ClientTransportPlugin::<AppTransportConfig, WebTransportClient<_>>::default(),
        ))
        .init_resource::<AsyncRuntime>()
        .add_systems(Startup, setup)
        .add_systems(Update, reply)
        .run();
}

fn setup(mut commands: Commands, rt: Res<AsyncRuntime>) {
    match create(rt.as_ref()) {
        Ok(client) => {
            commands.insert_resource(client);
            info!("Created client");
        }
        Err(err) => error!("Failed to create client: {err:#}"),
    }
}

fn create(rt: &AsyncRuntime) -> Result<WebTransportClient<AppTransportConfig>> {
    let config = ClientConfig::builder()
        .with_bind_default()
        .with_no_cert_validation()
        .keep_alive_interval(Some(Duration::from_secs(5)))
        .build();

    let (front, back) = aeronet_wt_native::create_client(config, TransportStreams::default());
    rt.0.spawn(async move {
        back.start().await.unwrap();
    });
    front.connect("https://[::1]:25565");
    Ok(front)
}

fn reply(
    mut connected: EventReader<LocalClientConnected>,
    mut disconnected: EventReader<LocalClientDisconnected>,
) {
    for LocalClientConnected in connected.iter() {
        info!("Client connected");
    }

    for LocalClientDisconnected { reason } in disconnected.iter() {
        info!(
            "Client disconnected: {:#}",
            aeronet::error::as_pretty(reason)
        );
    }
}
