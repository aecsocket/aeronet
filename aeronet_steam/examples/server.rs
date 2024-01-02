use std::{convert::Infallible, string::FromUtf8Error, time::Duration};

use aeronet::{LaneKey, Message, OnLane, TryAsBytes, TryFromBytes, TransportProtocol, LaneProtocol, ServerTransportPlugin, RemoteConnecting, RemoteConnected, FromClient, RemoteDisconnected, ServerTransport};
use aeronet_steam::SteamServerTransport;
use bevy::{prelude::*, app::ScheduleRunnerPlugin};

// Protocol

#[derive(Debug, Clone, LaneKey)]
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

impl LaneProtocol for AppProtocol {
    type Lane = AppLane;
}

// App

fn main() {
    App::new()
        .add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(100))),
            ServerTransportPlugin::<AppProtocol, SteamServerTransport<_>>::default(),
        ))
        .add_systems(Startup, setup)
        .add_systems(Update, (update_steam, update_server))
        .run();
}

fn setup(world: &mut World) {
    let (steam, steam_single) = steamworks::Client::init_app(480).unwrap();
    world.insert_non_send_resource(steam_single);
    
    let server = SteamServerTransport::<AppProtocol>::open_new_p2p(&steam, 0).unwrap();
    world.insert_resource(server);
    info!("Started server");
}

fn update_steam(steam: NonSend<steamworks::SingleClient>) {
    steam.run_callbacks();
}

fn update_server(
    mut server: ResMut<SteamServerTransport<AppProtocol>>,
    mut connecting: EventReader<RemoteConnecting>,
    mut connected: EventReader<RemoteConnected>,
    mut disconnected: EventReader<RemoteDisconnected<AppProtocol, SteamServerTransport<AppProtocol>>>,
    mut recv: EventReader<FromClient<AppProtocol>>,
) {
    for RemoteConnecting { client } in connecting.read() {
        info!("Client {client} connecting");
    }

    for RemoteConnected { client } in connected.read() {
        info!("Client {client} connected");
    }

    for RemoteDisconnected { client, reason } in disconnected.read() {
        info!("Client {client} disconnected: {reason:#}");
    }

    for FromClient { client, msg, .. } in recv.read() {
        info!("{client} > {}", msg.0);

        let resp = format!("You sent: {}", msg.0);
        let _ = server.send(*client, AppMessage(resp));
    }
}
