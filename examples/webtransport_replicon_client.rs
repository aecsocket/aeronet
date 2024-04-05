use aeronet::{
    lane::{LaneKey, OnLane},
    message::Message,
    protocol::TransportProtocol,
};
use aeronet_channel::{client::ChannelClient, server::ChannelServer};
use aeronet_replicon::{
    client::RepliconClientPlugin, protocol::RepliconMessage, server::RepliconServerPlugin,
};
use bevy::prelude::*;
use bevy_replicon::{
    core::{
        common_conditions::{has_authority, server_running},
        replication_rules::{AppReplicationExt, Replication},
        replicon_channels::ChannelKind,
        ClientId,
    },
    network_event::client_event::{ClientEventAppExt, FromClient},
    server::ServerEvent,
    RepliconPlugins,
};
use serde::{Deserialize, Serialize};

//
// protocol config
//

#[derive(Debug, Clone, Copy, LaneKey)]
#[lane_kind(ReliableOrdered)]
struct AppLane;

#[derive(Debug, Clone, Message, OnLane)]
#[on_lane(AppLane)]
struct AppMessage(String);

impl<T: Into<String>> From<T> for AppMessage {
    fn from(value: T) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, Copy, TransportProtocol)]
#[c2s(RepliconMessage)]
#[s2c(RepliconMessage)]
struct AppProtocol;

type Client = ChannelClient<AppProtocol>;
type Server = ChannelServer<AppProtocol>;

//
// world config
//

const MOVE_SPEED: f32 = 300.0;

#[derive(Debug, Clone, Serialize, Deserialize, Component)]
struct Player(ClientId);

#[derive(Debug, Clone, Default, Deref, DerefMut, Serialize, Deserialize, Component)]
struct PlayerPosition(Vec2);

#[derive(Debug, Clone, Default, Serialize, Deserialize, Component)]
struct PlayerColor(Color);

#[derive(Debug, Clone, Default, Serialize, Deserialize, Event)]
struct MoveDirection(Vec2);

#[derive(Debug, Bundle)]
struct PlayerBundle {
    player: Player,
    position: PlayerPosition,
    color: PlayerColor,
    replication: Replication,
}

impl PlayerBundle {
    fn new(id: ClientId, position: Vec2, color: Color) -> Self {
        Self {
            player: Player(id),
            position: PlayerPosition(position),
            color: PlayerColor(color),
            replication: Replication,
        }
    }
}

//
// logic
//

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            RepliconPlugins,
            RepliconServerPlugin::<AppProtocol, Server>::default(),
            RepliconClientPlugin::<AppProtocol, Client>::default(),
        ))
        .replicate::<PlayerPosition>()
        .replicate::<PlayerColor>()
        .add_client_event::<MoveDirection>(ChannelKind::Ordered)
        .add_systems(
            Startup,
            ((setup_server, setup_client).chain(), spawn_camera),
        )
        .add_systems(
            Update,
            (
                apply_movement,
                handle_connections.run_if(server_running),
                draw_boxes,
                read_input,
            ),
        )
        .run();
}

fn setup_server(mut commands: Commands) {
    let server = Server::open();
    commands.insert_resource(server);

    commands.spawn(PlayerBundle::new(
        ClientId::SERVER,
        Vec2::ZERO,
        Color::GREEN,
    ));
}

fn setup_client(mut commands: Commands, mut server: ResMut<Server>) {
    let client = Client::connect_new(&mut server);
    commands.insert_resource(client);

    commands.spawn(PlayerBundle::new(
        ClientId::SERVER,
        Vec2::ZERO,
        Color::GREEN,
    ));
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn(Camera2dBundle::default());
}

fn handle_connections(mut commands: Commands, mut server_events: EventReader<ServerEvent>) {
    for event in server_events.read() {
        match event {
            ServerEvent::ClientConnected { client_id } => {
                info!("{client_id:?} connected");
                // Generate pseudo random color from client id.
                let r = ((client_id.get() % 23) as f32) / 23.0;
                let g = ((client_id.get() % 27) as f32) / 27.0;
                let b = ((client_id.get() % 39) as f32) / 39.0;
                commands.spawn(PlayerBundle::new(
                    *client_id,
                    Vec2::ZERO,
                    Color::rgb(r, g, b),
                ));
            }
            ServerEvent::ClientDisconnected { client_id, reason } => {
                info!("{client_id:?} disconnected: {reason}");
            }
        }
    }
}

fn draw_boxes(mut gizmos: Gizmos, players: Query<(&PlayerPosition, &PlayerColor)>) {
    for (position, color) in &players {
        gizmos.rect(
            Vec3::new(position.x, position.y, 0.0),
            Quat::IDENTITY,
            Vec2::ONE * 50.0,
            color.0,
        );
    }
}

fn read_input(mut move_events: EventWriter<MoveDirection>, input: Res<ButtonInput<KeyCode>>) {
    let mut direction = Vec2::ZERO;
    if input.pressed(KeyCode::ArrowRight) {
        direction.x += 1.0;
    }
    if input.pressed(KeyCode::ArrowLeft) {
        direction.x -= 1.0;
    }
    if input.pressed(KeyCode::ArrowUp) {
        direction.y += 1.0;
    }
    if input.pressed(KeyCode::ArrowDown) {
        direction.y -= 1.0;
    }
    if direction != Vec2::ZERO {
        move_events.send(MoveDirection(direction.normalize_or_zero()));
    }
}

fn apply_movement(
    time: Res<Time>,
    mut move_events: EventReader<FromClient<MoveDirection>>,
    mut players: Query<(&Player, &mut PlayerPosition)>,
) {
    for FromClient { client_id, event } in move_events.read() {
        info!("received event {event:?} from {client_id:?}");
        for (player, mut position) in &mut players {
            if *client_id == player.0 {
                **position += event.0 * time.delta_seconds() * MOVE_SPEED;
            }
        }
    }
}
