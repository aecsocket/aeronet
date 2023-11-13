use std::{convert::Infallible, string::FromUtf8Error, time::Duration};

use aeronet::{AsyncRuntime, TryFromBytes, TryIntoBytes};
use aeronet_wt_native::{
    Channels, Disconnected, OnChannel, Opening, Transition, WebTransportClient,
};
use bevy::{log::LogPlugin, prelude::*};
use wtransport::ClientConfig;

// config

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Channels)]
#[channel_kind(Datagram)]
struct AppChannel;

#[derive(Debug, Clone, PartialEq, Eq, Hash, OnChannel)]
#[channel_type(AppChannel)]
#[on_channel(AppChannel)]
struct AppMessage(String);

impl<T> From<T> for AppMessage
where
    T: Into<String>,
{
    fn from(value: T) -> Self {
        Self(value.into())
    }
}

impl TryFromBytes for AppMessage {
    type Error = FromUtf8Error;

    fn try_from_bytes(buf: &[u8]) -> Result<Self, Self::Error> {
        String::from_utf8(buf.to_owned().into_iter().collect()).map(AppMessage)
    }
}

impl TryIntoBytes for AppMessage {
    type Error = Infallible;

    fn try_into_bytes(self) -> Result<Vec<u8>, Self::Error> {
        Ok(self.0.into_bytes())
    }
}

// logic

fn main() {
    App::new()
        .add_plugins((DefaultPlugins.set(LogPlugin {
            level: tracing::Level::DEBUG,
            ..default()
        }),))
        .init_resource::<AsyncRuntime>()
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands, rt: Res<AsyncRuntime>) {
    let config = ClientConfig::builder()
        .with_bind_default()
        .with_no_cert_validation()
        .keep_alive_interval(Some(Duration::from_secs(5)))
        .build();

    let (client, backend) = Opening::new(config);
    rt.0.spawn(backend.start());
    commands.insert_resource(WebTransportClient::from(client));
}

fn update(mut client: ResMut<WebTransportClient>) {
    match client {
        WebTransportClient::Creating(state) => {
            *client = match state.poll() {
                Transition::Pending(state) => WebTransportClient::from(state),
                Transition::Ready(Ok(state)) => WebTransportClient::from(state),
            }
        }
    }
}
