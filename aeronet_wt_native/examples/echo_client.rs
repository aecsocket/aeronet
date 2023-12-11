//!

use std::{convert::Infallible, string::FromUtf8Error, time::Duration};

use aeronet::{AsyncRuntime, ChannelKey, OnChannel, TryFromBytes, TryIntoBytes};
use anyhow::Result;
use bevy::{log::LogPlugin, prelude::*};
use wtransport::ClientConfig;

// config

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ChannelKey)]
#[channel_kind(Unreliable)]
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

impl TryIntoBytes for AppMessage {
    type Output<'a> = &'a [u8];

    type Error = Infallible;

    fn try_into_bytes(&self) -> Result<Self::Output<'_>, Self::Error> {
        Ok(self.0.as_bytes())
    }
}

impl TryFromBytes for AppMessage {
    type Error = FromUtf8Error;

    fn try_from_bytes(buf: &[u8]) -> Result<Self, Self::Error> {
        String::from_utf8(buf.to_owned().into_iter().collect()).map(AppMessage)
    }
}

type WebTransportClient = aeronet_wt_native::WebTransportClient<AppMessage, AppMessage, AppChannel>;

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
    match create(rt.as_ref()) {
        Ok(client) => {
            info!("Created client");
            commands.insert_resource(client);
        }
        Err(err) => panic!("Failed to create client: {err:#}"),
    }

    let (client, backend) = Opening::new(config);
    rt.0.spawn(backend.start());
    commands.insert_resource(WebTransportClient::from(client));
}

fn create(rt: &AsyncRuntime) -> Result<WebTransportClient> {
    let config = ClientConfig::builder()
        .with_bind_default()
        .with_no_cert_validation()
        .keep_alive_interval(Some(Duration::from_secs(5)))
        .build();

    let (client, backend) = WebTransportClient::Disconnected;
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
