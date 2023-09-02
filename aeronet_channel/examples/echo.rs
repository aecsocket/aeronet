use std::time::Duration;

use aeronet::{
    ClientId, ClientSet, ClientTransportPlugin, Message, ServerTransportEvent,
    ServerTransportPlugin, TransportSettings, ServerTransportError,
};
use aeronet_channel::{ChannelClientTransport, ChannelServerTransport};
use bevy::{app::ScheduleRunnerPlugin, prelude::*};

pub struct AppTransportSettings;

#[derive(Debug, Clone)]
pub enum C2S {
    Ping(String),
}

impl Message for C2S {}

#[derive(Debug, Clone)]
pub enum S2C {
    Pong(String),
}

impl Message for S2C {}

impl TransportSettings for AppTransportSettings {
    type C2S = C2S;
    type S2C = S2C;
}

// Since your app will most likely only use one type of transport and one type of settings,
// it's recommended to typedef your desired transport and your app's transport settings together

pub type ClientTransport = ChannelClientTransport<AppTransportSettings>;

pub type ServerTransport = ChannelServerTransport<AppTransportSettings>;

pub type ClientRecvEvent = aeronet::ClientRecvEvent<AppTransportSettings>;

pub type ClientSendEvent = aeronet::ClientSendEvent<AppTransportSettings>;

pub type ServerRecvEvent = aeronet::ServerRecvEvent<AppTransportSettings>;

pub type ServerSendEvent = aeronet::ServerSendEvent<AppTransportSettings>;

fn main() {
    App::new()
        .add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(100))))
        .add_plugins((
            ClientTransportPlugin::<AppTransportSettings, ClientTransport>::default(),
            ServerTransportPlugin::<AppTransportSettings, ServerTransport>::default(),
        ))
        .add_systems(Startup, setup)
        .add_systems(Update, (update_client, update_server).chain())
        .insert_resource(PingTimer(Timer::new(
            Duration::from_millis(500),
            TimerMode::Repeating,
        )))
        .add_systems(Update, send_ping)
        .add_systems(Update, disconnect.run_if(should_disconnect))
        .run();
}

fn setup(mut commands: Commands) {
    let mut server_tx = ServerTransport::new();
    let (client_tx, client_id) = server_tx.connect();

    commands.insert_resource(server_tx);
    commands.insert_resource(ClientSet::default());

    commands.insert_resource(client_tx);
    commands.insert_resource(ConnectedClientId(client_id));
}

fn update_client(mut recv: EventReader<ClientRecvEvent>) {
    for ClientRecvEvent { msg } in recv.iter() {
        println!("[cl] Received {:?}", msg);
    }
}

fn update_server(
    mut recv: EventReader<ServerRecvEvent>,
    mut events: EventReader<ServerTransportEvent>,
    mut errors: EventReader<ServerTransportError>,
    mut send: EventWriter<ServerSendEvent>,
) {
    for event in events.iter() {
        println!("[sv] Event: {:?}", event);
    }

    for err in errors.iter() {
        println!("[sv] Error: {:#}", err);
    }

    for ServerRecvEvent { from, msg } in recv.iter() {
        println!("[sv] Received {:?}", msg);
        match msg {
            C2S::Ping(text) => {
                println!("[sv] Sending pong");
                send.send(ServerSendEvent {
                    to: *from,
                    msg: S2C::Pong(text.clone()),
                });
            }
        }
    }
}

#[derive(Resource)]
pub struct ConnectedClientId(ClientId);

#[derive(Resource)]
pub struct PingTimer(Timer);

fn send_ping(
    mut send: EventWriter<ClientSendEvent>,
    time: Res<Time>,
    mut timer: ResMut<PingTimer>,
) {
    timer.0.tick(time.delta());
    if timer.0.just_finished() {
        timer.0.reset();
        let msg = C2S::Ping(format!("Time is {}", time.elapsed_seconds()));
        println!("[cl] Sending ping");
        send.send(ClientSendEvent { msg });
    }
}

fn should_disconnect(time: Res<Time>) -> bool {
    time.elapsed_seconds() > 5.0
}

fn disconnect(mut server_tx: ResMut<ServerTransport>, client_id: Res<ConnectedClientId>) {
    println!("[cl] Disconnecting");
    server_tx.disconnect(client_id.0);
}
