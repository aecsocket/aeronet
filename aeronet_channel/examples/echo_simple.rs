use std::time::Duration;

use aeronet::{
    ClientId, ClientSet, ClientTransportPlugin, ServerTransportEvent, ServerTransportPlugin,
    TransportSettings,
};
use aeronet_channel::{ChannelClientTransport, ChannelServerTransport};
use bevy::{app::ScheduleRunnerPlugin, log::LogPlugin, prelude::*};

pub struct AppTransportSettings;

#[derive(Debug, Clone)]
pub enum C2S {
    Ping(String),
}

#[derive(Debug, Clone)]
pub enum S2C {
    Pong(String),
}

impl TransportSettings for AppTransportSettings {
    type C2S = C2S;
    type S2C = S2C;
}

// Since your app will most likely only use one type of transport and one type of settings,
// it's recommended to typedef your desired transport and your app's transport settings together

pub type AppClientTransport = ChannelClientTransport<AppTransportSettings>;
pub type AppServerTransport = ChannelServerTransport<AppTransportSettings>;

pub type ClientRecvEvent = aeronet::ClientRecvEvent<AppTransportSettings>;
pub type ClientSendEvent = aeronet::ClientSendEvent<AppTransportSettings>;

pub type ServerRecvEvent = aeronet::ServerRecvEvent<AppTransportSettings>;
pub type ServerSendEvent = aeronet::ServerSendEvent<AppTransportSettings>;

fn main() {
    App::new()
        .add_plugins((
            LogPlugin::default(),
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(100))),
        ))
        .add_plugins((
            ClientTransportPlugin::<AppTransportSettings, AppClientTransport>::default(),
            ServerTransportPlugin::<AppTransportSettings, AppServerTransport>::default(),
        ))
        .add_systems(Startup, setup)
        //
        .add_systems(
            Update,
            client::update.run_if(resource_exists::<AppClientTransport>()),
        )
        .add_systems(Update, server::update)
        // send pings
        .insert_resource(client::PingTimer(Timer::new(
            Duration::from_millis(500),
            TimerMode::Repeating,
        )))
        .add_systems(Update, client::send_ping)
        .add_systems(Update, server::disconnect.run_if(server::should_disconnect))
        .run();
}

fn setup(mut commands: Commands) {
    let mut server_tx = AppServerTransport::new();
    let (client_tx, client_id) = server_tx.connect();

    commands.insert_resource(server_tx);
    commands.insert_resource(ClientSet::default());

    commands.insert_resource(client_tx);
    commands.insert_resource(ConnectedClientId(client_id));
}

#[derive(Resource)]
pub struct ConnectedClientId(ClientId);

mod client {
    use aeronet::ClientTransportEvent;

    use super::*;

    pub fn update(
        mut events: EventReader<ClientTransportEvent>,
        mut recv: EventReader<ClientRecvEvent>,
    ) {
        for event in events.iter() {
            info!("Event: {:?}", event);
        }

        for ClientRecvEvent { msg } in recv.iter() {
            info!("Received {:?}", msg);
        }
    }

    #[derive(Resource)]
    pub struct PingTimer(pub Timer);

    pub fn send_ping(
        mut send: EventWriter<ClientSendEvent>,
        time: Res<Time>,
        mut timer: ResMut<PingTimer>,
    ) {
        timer.0.tick(time.delta());
        if timer.0.just_finished() {
            timer.0.reset();
            let msg = C2S::Ping(format!("Time is {}", time.elapsed_seconds()));
            info!("Sending ping");
            send.send(ClientSendEvent { msg });
        }
    }
}

mod server {
    use aeronet::ServerDisconnectClientEvent;

    use super::*;

    pub fn update(
        mut recv: EventReader<ServerRecvEvent>,
        mut events: EventReader<ServerTransportEvent>,
        mut send: EventWriter<ServerSendEvent>,
    ) {
        for event in events.iter() {
            info!("Event: {:?}", event);
        }

        for ServerRecvEvent { from, msg } in recv.iter() {
            info!("Received {:?}", msg);
            match msg {
                C2S::Ping(text) => {
                    info!("Sending pong");
                    send.send(ServerSendEvent {
                        to: *from,
                        msg: S2C::Pong(text.clone()),
                    });
                }
            }
        }
    }

    pub fn should_disconnect(time: Res<Time>, client: Option<Res<ConnectedClientId>>) -> bool {
        time.elapsed_seconds() > 3.0 && client.is_some()
    }

    pub fn disconnect(
        mut commands: Commands,
        mut disconnect: EventWriter<ServerDisconnectClientEvent>,
        client_id: Res<ConnectedClientId>,
    ) {
        info!("Disconnecting");
        disconnect.send(ServerDisconnectClientEvent {
            client: client_id.0,
        });
        commands.remove_resource::<ConnectedClientId>();
    }
}
