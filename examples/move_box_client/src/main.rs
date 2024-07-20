#![doc = include_str!("../README.md")]

use aeronet::{
    client::{ClientState, ClientTransport, LocalClientConnected, LocalClientDisconnected},
    error::pretty_error,
    server::ServerTransportSet,
};
use aeronet_proto::{
    stats::{ClientSessionStats, ClientSessionStatsPlugin},
    visualizer::SessionStatsVisualizer,
};
use aeronet_replicon::client::RepliconClientPlugin;
use aeronet_webtransport::{client::WebTransportClient, wtransport};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPlugin};
use bevy_replicon::prelude::RepliconChannels;
use move_box::{AsyncRuntime, MoveBoxPlugin, PlayerColor, PlayerMove, PlayerPosition};
use web_time::Duration;

/// `move_box` demo client
#[derive(Debug, Resource, clap::Parser)]
struct Args {
    /// URL of the server to connect to
    target: String,
}

impl FromWorld for Args {
    fn from_world(_: &mut World) -> Self {
        <Self as clap::Parser>::parse()
    }
}

fn main() {
    App::new()
        .init_resource::<Args>()
        .add_plugins((
            DefaultPlugins,
            RepliconClientPlugin::<WebTransportClient>::default(),
            MoveBoxPlugin,
            ClientSessionStatsPlugin::<WebTransportClient>::default(),
            EguiPlugin,
        ))
        .init_resource::<WebTransportClient>()
        .init_resource::<SessionStatsVisualizer>()
        .add_systems(Startup, (setup_level, connect_client).chain())
        .add_systems(
            PreUpdate,
            (on_connected, on_disconnected).after(ServerTransportSet::Recv),
        )
        .add_systems(Update, (handle_inputs, draw_boxes, draw_stats).chain())
        .run();
}

fn setup_level(mut commands: Commands) {
    commands.spawn(Camera2dBundle::default());
}

fn connect_client(
    mut client: ResMut<WebTransportClient>,
    tasks: Res<AsyncRuntime>,
    args: Res<Args>,
    channels: Res<RepliconChannels>,
) {
    let net_config = wtransport::ClientConfig::builder()
        .with_bind_default()
        .with_no_cert_validation()
        .keep_alive_interval(Some(Duration::from_secs(1)))
        .max_idle_timeout(Some(Duration::from_secs(5)))
        .unwrap()
        .build();

    let session_config = move_box::base_session_config()
        .with_send_lanes(channels.client_channels())
        .with_recv_lanes(channels.server_channels());

    let backend = client
        .connect(net_config, session_config, args.target.clone())
        .unwrap();
    tasks.spawn(backend);
}

fn on_connected(mut events: EventReader<LocalClientConnected<WebTransportClient>>) {
    for LocalClientConnected { .. } in events.read() {
        info!("Client connected");
    }
}

fn on_disconnected(mut events: EventReader<LocalClientDisconnected<WebTransportClient>>) {
    for LocalClientDisconnected { error } in events.read() {
        info!("Client disconnected: {:#}", pretty_error(&error));
    }
}

fn handle_inputs(mut move_events: EventWriter<PlayerMove>, input: Res<ButtonInput<KeyCode>>) {
    let mut delta = Vec2::ZERO;
    if input.pressed(KeyCode::ArrowRight) {
        delta.x += 1.0;
    }
    if input.pressed(KeyCode::ArrowLeft) {
        delta.x -= 1.0;
    }
    if input.pressed(KeyCode::ArrowUp) {
        delta.y += 1.0;
    }
    if input.pressed(KeyCode::ArrowDown) {
        delta.y -= 1.0;
    }
    if delta != Vec2::ZERO {
        // don't normalize here, since that means it's client sided
        // normalize the delta on the server side
        move_events.send(PlayerMove(delta));
    }
}

fn draw_boxes(mut gizmos: Gizmos, players: Query<(&PlayerPosition, &PlayerColor)>) {
    for (PlayerPosition(pos), PlayerColor(color)) in &players {
        gizmos.rect(pos.extend(0.0), Quat::IDENTITY, Vec2::ONE * 50.0, *color);
    }
}

fn draw_stats(
    mut egui: EguiContexts,
    client: Res<WebTransportClient>,
    stats: Res<ClientSessionStats<WebTransportClient>>,
    mut stats_visualizer: ResMut<SessionStatsVisualizer>,
) {
    if let ClientState::Connected(client) = client.state() {
        stats_visualizer.draw(egui.ctx_mut(), &client.session, &*stats);
    }
}
