// https://github.com/projectharmonia/bevy_replicon/blob/master/bevy_replicon_renet/examples/simple_box.rs

use aeronet::{
    client::{client_disconnected, ClientTransport},
    protocol::{ProtocolVersion, TransportProtocol},
};
use aeronet_replicon::{
    channel::RepliconChannelsExt, client::RepliconClientPlugin, protocol::RepliconMessage,
};
use aeronet_webtransport::{
    client::{ClientConfig, WebTransportClient},
    shared::WebTransportProtocol,
};
use bevy::{log::LogPlugin, prelude::*};
use bevy_ecs::system::SystemId;
use bevy_egui::{egui, EguiContexts, EguiPlugin};
use bevy_replicon::prelude::*;
use serde::{Deserialize, Serialize};

//
// transport config
//

#[derive(Debug, Clone, Copy)]
struct AppProtocol;

impl TransportProtocol for AppProtocol {
    type C2S = RepliconMessage;
    type S2C = RepliconMessage;
}

impl WebTransportProtocol for AppProtocol {
    type Mapper = ();
}

const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion(0xbabad0d0bebebaba);

type Client = WebTransportClient<AppProtocol>;

//
// world config
//

const MOVE_SPEED: f32 = 200.0;

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

fn main() {
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
            EguiPlugin,
            RepliconPlugins.build().disable::<ServerPlugin>(),
            RepliconClientPlugin::<_, Client>::default(),
        ))
        .init_resource::<Client>()
        .replicate::<PlayerPosition>()
        .replicate::<PlayerColor>()
        .add_client_event::<MoveDirection>(ChannelKind::Unreliable)
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                ui,
                apply_movement.run_if(has_authority),
                draw_boxes,
                read_input,
                clean_up.run_if(client_disconnected::<AppProtocol, Client>),
            ),
        )
        .run();
}

fn setup(world: &mut World) {
    #[cfg(not(target_family = "wasm"))]
    world.init_resource::<aeronet::bevy_tokio_rt::TokioRuntime>();
    world.spawn(Camera2dBundle::default());

    let connect = Connect(world.register_system(connect));
    world.insert_resource(connect);
}

#[derive(Debug, Clone, Resource, Deref, DerefMut)]
struct Connect(SystemId<String>);

#[cfg(target_family = "wasm")]
fn connect(In(target): In<String>, mut client: ResMut<Client>, channels: Res<RepliconChannels>) {
    use xwt::current::WebTransportOptions;

    let native_config = WebTransportOptions::default();
    let config = ClientConfig {
        version: PROTOCOL_VERSION,
        lanes_in: channels.to_s2c_lanes(),
        lanes_out: channels.to_c2s_lanes(),
        ..ClientConfig::new(native_config, ())
    };
    let Ok(backend) = client.connect(config, target) else {
        return;
    };
    wasm_bindgen_futures::spawn_local(backend);
}

#[cfg(not(target_family = "wasm"))]
fn connect(
    In(target): In<String>,
    rt: Res<aeronet::bevy_tokio_rt::TokioRuntime>,
    mut client: ResMut<Client>,
    channels: Res<RepliconChannels>,
) {
    let native_config = aeronet_webtransport::wtransport::ClientConfig::builder()
        .with_bind_default()
        .with_no_cert_validation()
        .keep_alive_interval(Some(std::time::Duration::from_secs(5)))
        .build();
    let config = ClientConfig {
        version: PROTOCOL_VERSION,
        lanes_in: channels.to_s2c_lanes(),
        lanes_out: channels.to_c2s_lanes(),
        ..ClientConfig::new(native_config, ())
    };
    let Ok(backend) = client.connect(config, target) else {
        return;
    };
    rt.spawn(backend);
}

fn ui(
    mut commands: Commands,
    mut egui: EguiContexts,
    mut url_buf: Local<String>,
    mut client: ResMut<Client>,
    connect: Res<Connect>,
) {
    egui::Window::new("Connection").show(egui.ctx_mut(), |ui| {
        ui.add_enabled_ui(client.state().is_disconnected(), |ui| {
            ui.horizontal(|ui| {
                ui.label("URL");
                ui.text_edit_singleline(&mut *url_buf);
            });

            if ui.button("Connect").clicked() {
                commands.run_system_with_input(**connect, std::mem::take(&mut url_buf));
            }
        });

        ui.add_enabled_ui(!client.state().is_disconnected(), |ui| {
            if ui.button("Disconnect").clicked() {
                let _ = client.disconnect();
            }
        })
    });
}

//
// app
//

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
        for (player, mut position) in &mut players {
            if *client_id == player.0 {
                **position += event.0 * time.delta_seconds() * MOVE_SPEED;
            }
        }
    }
}

fn clean_up(mut commands: Commands, players: Query<Entity, With<Player>>) {
    for entity in &players {
        commands.entity(entity).despawn();
    }
}
