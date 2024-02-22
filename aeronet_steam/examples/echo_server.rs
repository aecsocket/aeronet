use std::{convert::Infallible, string::FromUtf8Error, time::Duration};

use aeronet::{
    client::ClientState,
    server::{
        FromClient, RemoteClientConnected, RemoteClientConnecting, RemoteClientDisconnected,
        ServerClosed, ServerOpened, ServerTransport, ServerTransportPlugin,
    },
    LaneKey, Message, OnLane, ProtocolVersion, TransportProtocol, TryAsBytes, TryFromBytes,
};
use aeronet_steam::{ListenTarget, SteamServerTransportConfig, MTU};
use bevy::{app::ScheduleRunnerPlugin, log::LogPlugin, prelude::*};
use steamworks::ClientManager;

// Protocol

#[derive(Debug, Clone, Copy, LaneKey)]
#[lane_kind(UnreliableSequenced)]
struct AppLane;

#[derive(Debug, Clone, Message, OnLane)]
#[lane_type(AppLane)]
#[on_lane(AppLane)]
struct AppMessage(String);

impl<T: Into<String>> From<T> for AppMessage {
    fn from(value: T) -> Self {
        Self(value.into())
    }
}

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
            LogPlugin {
                filter: "wgpu=error,naga=warn,aeronet=debug".into(),
                ..default()
            },
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(100))),
            ServerTransportPlugin::<_, Server>::default(),
        ))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                update_steam,
                on_opened,
                on_closed,
                on_incoming,
                on_connected,
                on_disconnected,
                on_recv,
            ),
        )
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

fn on_opened(mut events: EventReader<ServerOpened<AppProtocol, Server>>) {
    for ServerOpened { .. } in events.read() {
        info!("Opened server for connections");
    }
}

fn on_closed(mut events: EventReader<ServerClosed<AppProtocol, Server>>) {
    for ServerClosed { reason } in events.read() {
        info!("Server closed: {:#}", aeronet::util::pretty_error(&reason))
    }
}

fn on_incoming(
    mut events: EventReader<RemoteClientConnecting<AppProtocol, Server>>,
    mut server: ResMut<Server>,
) {
    for RemoteClientConnecting { client, .. } in events.read() {
        // Once the server sends out an event saying that a client is connecting
        // (`RemoteConnecting`) you can get its `client_state` and read its
        // connection info, to decide if you want to accept or reject it.
        if let ClientState::Connecting(info) = server.client_state(*client) {
            info!("Client {client} incoming ({:?})", info.steam_id,);
        }
        // IMPORTANT NOTE: You must either accept or reject the request after
        // receiving it. You don't have to do it immediately, but you do
        // have to do it eventually - the sooner the better.
        let _ = server.accept_request(*client);
    }
}

fn on_connected(
    mut events: EventReader<RemoteClientConnected<AppProtocol, Server>>,
    mut server: ResMut<Server>,
) {
    for RemoteClientConnected { client, .. } in events.read() {
        if let ClientState::Connected(info) = server.client_state(*client) {
            info!("Client {client} connected ({:?})", info.steam_id,);
        };
        let _ = server.send(*client, "Welcome!");
        let _ = server.send(*client, "Send me some UTF-8 text, and I will send it back");
    }
}

fn on_disconnected(mut events: EventReader<RemoteClientDisconnected<AppProtocol, Server>>) {
    for RemoteClientDisconnected { client, reason } in events.read() {
        info!(
            "Client {client} disconnected: {:#}",
            aeronet::util::pretty_error(reason)
        );
    }
}

fn on_recv(mut events: EventReader<FromClient<AppProtocol, Server>>, mut server: ResMut<Server>) {
    for FromClient { client, msg, .. } in events.read() {
        info!("{client} > {}", msg.0);
        let resp = format!("You sent: {}", msg.0);
        info!("{client} < {resp}");
        let _ = server.send(*client, AppMessage(resp));
    }
}
