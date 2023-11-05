use aeronet::{
    ClientId, ClientTransportPlugin, FromClient, FromServer, LocalClientConnected,
    LocalClientDisconnected, RemoteClientConnected, RemoteClientDisconnected,
    ServerTransportPlugin, ToClient, ToServer, TryFromBytes, TryIntoBytes,
};
use aeronet_channel::{ChannelTransportClient, ChannelTransportServer, ClientState, ChannelServer, Disconnected, Connected, ChannelClient};
use anyhow::Result;
use bevy::{log::LogPlugin, prelude::*};
use bevy_egui::{egui, EguiContexts, EguiPlugin};

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
            .map_err(Into::into)
    }
}

type Client = ChannelTransportClient<AppMessage, AppMessage>;
type Server = ChannelTransportServer<AppMessage, AppMessage>;

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
        .add_plugins((
            DefaultPlugins.set(LogPlugin {
                level: tracing::Level::DEBUG,
                ..default()
            }),
        ))
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands) {
    // create a disconnected client
    // this client can only `connect`, no `recv` or `disconnect` available
    let client: Client<AppMessage, AppMessage, Disconnected> = Client::<_, _, _>::new();

    // connect this client to a server
    let mut server: ChannelServer<AppMessage, AppMessage> = ChannelServer::new();
    let client: Client<AppMessage, AppMessage, Connected<_, _>> = client.connect(&mut server);
    // can now `disconnect` and `recv`

    // use `recv` to get back a tuple of:
    // * an enum which holds either a Connected or Disconnected client
    //   since `recv` might pick up if the client is disconnected, in which case
    //   it gives back a disconnected client
    // * an iterator of the events
    let (client, events): (ChannelClient<_, _>, _) = client.recv();

    if let ChannelClient::Connected(client) = client {
        // type-safe disconnect
        client.disconnect();
    }
}
