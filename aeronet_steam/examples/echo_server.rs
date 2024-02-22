use std::{convert::Infallible, string::FromUtf8Error, time::Duration};

use aeronet::{
    client::ClientState,
    server::{
        FromClient, RemoteClientConnected, RemoteClientConnecting, RemoteClientDisconnected,
        ServerTransport, ServerTransportPlugin,
    },
    LaneKey, Message, OnLane, ProtocolVersion, TransportProtocol, TryAsBytes, TryFromBytes,
};
use aeronet_steam::{ListenTarget, SteamServerTransportConfig, MTU};
use bevy::{app::ScheduleRunnerPlugin, log::LogPlugin, prelude::*};
use steamworks::ClientManager;

// Protocol

#[derive(Debug, Clone, Copy, LaneKey)]
#[lane_kind(ReliableOrdered)]
struct AppLane;

#[derive(Debug, Clone, Message, OnLane)]
#[lane_type(AppLane)]
#[on_lane(AppLane)]
struct AppMessage(String);

impl TryAsBytes for AppMessage {
    type Output<'a> = &'a [u8];
    type Error = Infallible;

    fn try_as_bytes(&self) -> Result<Self::Output<'_>, Self::Error> {
        Ok(self.0.as_bytes())
    }
}

impl TryFromBytes for AppMessage {
    type Error = FromUtf8Error;

    fn try_from_bytes(buf: &[u8]) -> Result<Self, Self::Error> {
        String::from_utf8(buf.to_vec()).map(AppMessage)
    }
}

struct AppProtocol;

impl TransportProtocol for AppProtocol {
    type C2S = AppMessage;
    type S2C = AppMessage;
}

const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion(0xdeadbeefbadc0de);

// Use a `ClientManager` here since we use `steamworks::Client`, not
// `steamworks::Server`
type Server = aeronet_steam::SteamServerTransport<AppProtocol, ClientManager>;

// App

fn main() {
    App::new()
        .add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(100))),
            LogPlugin::default(),
            ServerTransportPlugin::<_, Server>::default(),
        ))
        .add_systems(Startup, setup)
        .add_systems(Update, (update_steam, update_server))
        .run();
}

fn setup(world: &mut World) {
    let (steam, steam_single) = steamworks::Client::init_app(480).unwrap();
    world.insert_non_send_resource(steam_single);

    let addr = "0.0.0.0:27015".parse().unwrap();
    let config = SteamServerTransportConfig {
        version: PROTOCOL_VERSION,
        max_packet_len: MTU,
        lanes: AppLane::config(),
        target: ListenTarget::Ip(addr),
    };
    let server = Server::open_new(&steam, config).unwrap();
    world.insert_resource(server);
    info!("Started server on {addr}");
}

fn update_steam(steam: NonSend<steamworks::SingleClient>) {
    steam.run_callbacks();
}

fn update_server(
    mut server: ResMut<Server>,
    mut connecting: EventReader<RemoteClientConnecting<AppProtocol, Server>>,
    mut connected: EventReader<RemoteClientConnected<AppProtocol, Server>>,
    mut disconnected: EventReader<RemoteClientDisconnected<AppProtocol, Server>>,
    mut recv: EventReader<FromClient<AppProtocol, Server>>,
) {
    for RemoteClientConnecting { client, .. } in connecting.read() {
        let ClientState::Connecting(info) = server.client_state(*client) else {
            panic!("client should be in connecting state");
        };
        info!("Client {client} connecting ({:?})", info.steam_id);
        let _ = server.accept_request(*client);
    }

    for RemoteClientConnected { client, .. } in connected.read() {
        info!("Client {client} connected");
    }

    for RemoteClientDisconnected { client, reason } in disconnected.read() {
        info!(
            "Client {client} disconnected: {:#}",
            aeronet::util::pretty_error(&reason)
        );
    }

    for FromClient { client, msg, .. } in recv.read() {
        info!("{client} > {}", msg.0);

        let resp = format!("You sent: {}", msg.0);
        let _ = server.send(*client, AppMessage(resp));
    }
}
