// https://github.com/projectharmonia/bevy_replicon/blob/master/bevy_replicon_renet/examples/simple_box.rs

use aeronet::{
    bevy_tokio_rt::TokioRuntime,
    client::LocalClientDisconnected,
    protocol::{ProtocolVersion, TransportProtocol},
};
use aeronet_replicon::{client::RepliconClientPlugin, protocol::RepliconMessage};
use aeronet_webtransport::{
    client::{ClientConfig, WebTransportClient},
    shared::WebTransportProtocol,
};
use bevy::{app::AppExit, log::LogPlugin, prelude::*};
use bevy_replicon::prelude::*;
use clap::Parser;
use serde::{Deserialize, Serialize};

//
// transport config
//

#[derive(Debug, Clone, Copy, TransportProtocol)]
#[c2s(RepliconMessage)]
#[s2c(RepliconMessage)]
struct AppProtocol;

impl WebTransportProtocol for AppProtocol {
    type Mapper = ();
}

const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion(0xbabad0d0bebebaba);

type Client = WebTransportClient<AppProtocol>;

//
// world config
//

const MOVE_SPEED: f32 = 300.0;

#[derive(Debug, Clone, Serialize, Deserialize, Component)]
struct Player(ClientId);

#[derive(Debug, Clone, Serialize, Deserialize, Deref, DerefMut, Component)]
struct PlayerPosition(Vec2);

#[derive(Debug, Clone, Serialize, Deserialize, Component)]
struct PlayerColor(Color);

/// A movement event for the controlled box.
#[derive(Debug, Clone, Serialize, Deserialize, Event)]
struct MoveDirection(Vec2);

//
// logic
//

/// WebTransport box demo client.
#[derive(Debug, Resource, clap::Parser)]
struct Args {
    /// URL to connect to, e.g. `https://[::1]:25565`.
    target: String,
}

fn main() {
    let args = Args::parse();
    App::new()
        .add_plugins((
            DefaultPlugins
                .set(LogPlugin {
                    filter: "wgpu=error,naga=warn,aeronet=debug".to_string(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Client".into(),
                        ..default()
                    }),
                    ..default()
                }),
            RepliconPlugins.build().disable::<ServerPlugin>(),
            RepliconClientPlugin::<_, Client>::default(),
        ))
        .insert_resource(args)
        .init_resource::<Client>()
        .replicate::<PlayerPosition>()
        .replicate::<PlayerColor>()
        .add_client_event::<MoveDirection>(ChannelKind::Ordered)
        .add_systems(Startup, (setup, connect).chain())
        .add_systems(
            Update,
            (
                apply_movement.run_if(has_authority),
                close.run_if(on_event::<LocalClientDisconnected<AppProtocol, Client>>()),
                draw_boxes,
                read_input,
            ),
        )
        .run();
}

fn setup(mut commands: Commands) {
    #[cfg(not(target_family = "wasm"))]
    commands.init_resource::<TokioRuntime>();
    commands.spawn(Camera2dBundle::default());
}

#[cfg(target_family = "wasm")]
fn connect(mut commands: Commands, args: Res<Args>, mut client: ResMut<Client>) {
    let native_config = aeronet_webtransport::web_sys::WebTransportOptions::new();
    let backend = client
        .connect(client_config(native_config), &args.target)
        .unwrap();
    wasm_bindgen_futures::spawn_local(backend);
}

#[cfg(not(target_family = "wasm"))]
fn connect(
    args: Res<Args>,
    rt: Res<TokioRuntime>,
    mut client: ResMut<Client>,
    channels: Res<RepliconChannels>,
) {
    use aeronet_replicon::channel::RepliconChannelsExt;

    let native_config = aeronet_webtransport::wtransport::ClientConfig::builder()
        .with_bind_default()
        .with_no_cert_validation()
        .keep_alive_interval(Some(std::time::Duration::from_secs(5)))
        .build();
    let config = ClientConfig {
        version: PROTOCOL_VERSION,
        lanes_in: channels.to_server_lanes(),
        lanes_out: channels.to_client_lanes(),
        ..ClientConfig::new(native_config, ())
    };
    let backend = client.connect(config, args.target.clone()).unwrap();
    rt.spawn(backend);
}

//
// replicon
//

fn close(mut exit: EventWriter<AppExit>) {
    exit.send(AppExit);
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
        info!("Sent input {direction}");
        move_events.send(MoveDirection(direction.normalize_or_zero()));
    }
}

fn apply_movement(
    time: Res<Time>,
    mut move_events: EventReader<FromClient<MoveDirection>>,
    mut players: Query<(&Player, &mut PlayerPosition)>,
) {
    for FromClient { client_id, event } in move_events.read() {
        for (player, mut position) in &mut players {
            if *client_id == player.0 {
                **position += event.0 * time.delta_seconds() * MOVE_SPEED;
            }
        }
    }
}
