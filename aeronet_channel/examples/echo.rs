use std::{string::FromUtf8Error, convert::Infallible};

use aeronet::{TryFromBytes, TryIntoBytes};
use aeronet_channel::{TransportClient, ChannelClient, ChannelServer};
use bevy::{log::LogPlugin, prelude::*};

// config

#[derive(Debug, Clone)]
pub struct AppMessage(pub String);

impl<T> From<T> for AppMessage
where
    T: Into<String>,
{
    fn from(value: T) -> Self {
        Self(value.into())
    }
}

impl TryIntoBytes for AppMessage {
    type Error = Infallible;

    fn try_into_bytes(self) -> Result<Vec<u8>, Self::Error> {
        Ok(self.0.into_bytes())
    }
}

impl TryFromBytes for AppMessage {
    type Error = FromUtf8Error;

    fn try_from_bytes(buf: &[u8]) -> Result<Self, Self::Error> {
        String::from_utf8(buf.to_owned().into_iter().collect()).map(|s| AppMessage(s))
    }
}

type Client = ChannelClient<AppMessage, AppMessage>;
type Server = ChannelServer<AppMessage, AppMessage>;

// resources

#[derive(Debug, Clone, Default, Resource)]
struct ClientState {
    scrollback: Vec<String>,
    buf: String,
}

#[derive(Debug, Clone, Default, Resource)]
struct ServerState {
    scrollback: Vec<String>,
    target_client: usize,
    buf: String,
}

// logic

fn main() {
    App::new()
        .add_plugins((DefaultPlugins.set(LogPlugin {
            level: tracing::Level::DEBUG,
            ..default()
        }),))
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands) {
    let mut server = Server::new();
    let client = Client::new().connect(&mut server);
    commands.insert_resource(server);
    commands.insert_resource(Client::from(client));
}

fn recv(mut client: ResMut<Client>) {
    client.send("abc");
}
