#![doc = include_str!("../README.md")]

use aeronet::{
    client::{ClientState, ClientTransport, LocalClientConnected, LocalClientDisconnected},
    error::pretty_error,
};
use aeronet_replicon::client::RepliconClientPlugin;
use aeronet_webtransport::{
    cert,
    client::{ClientConfig, WebTransportClient},
    proto::{
        session::SessionConfig,
        stats::{ClientSessionStats, ClientSessionStatsPlugin},
        visualizer::SessionStatsVisualizer,
    },
    runtime::WebTransportRuntime,
};
use bevy::{ecs::system::SystemId, prelude::*};
use bevy_egui::{egui, EguiContexts, EguiPlugin};
use bevy_replicon::{client::ClientSet, prelude::*};
use move_box::{GameState, MoveBoxPlugin, Player, PlayerColor, PlayerMove, PlayerPosition};

/// State of the [`egui`] interface used for connecting and disconnecting.
#[derive(Debug, Default, Resource)]
struct UiState {
    /// HTTPS URL of the server to connect to.
    target: String,
    /// Optional hash of a certificate that we want to ignore validation for.
    ///
    /// See the readme for why this is necessary.
    cert_hash: String,
}

/// One-shot system for connecting to a remote server.
///
/// Accepts a tuple of `(target, cert_hash)`.
#[derive(Debug, Clone, Copy, Deref, Resource)]
struct ConnectToRemote(SystemId<(String, String)>);

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            RepliconClientPlugin::<WebTransportClient>::default(),
            MoveBoxPlugin,
            ClientSessionStatsPlugin::<WebTransportClient>::default(),
            EguiPlugin,
        ))
        .init_resource::<WebTransportClient>()
        .init_resource::<SessionStatsVisualizer>()
        .init_resource::<UiState>()
        .add_systems(Startup, (setup_level, setup_systems))
        .add_systems(
            PreUpdate,
            (on_connected, on_disconnected, init_player).after(ClientSet::Receive),
        )
        .add_systems(
            Update,
            (
                ui,
                draw_stats,
                draw_boxes,
                handle_inputs.run_if(in_state(GameState::Playing)),
            ),
        )
        .run();
}

fn setup_level(mut commands: Commands) {
    commands.spawn(Camera2dBundle::default());
}

fn setup_systems(world: &mut World) {
    let connect_to_remote = world.register_system(connect_to_remote);
    world.insert_resource(ConnectToRemote(connect_to_remote));
}

fn connect_to_remote(
    In((target, spki_hash)): In<(String, String)>,
    mut client: ResMut<WebTransportClient>,
    channels: Res<RepliconChannels>,
    runtime: Res<WebTransportRuntime>,
) {
    let net_config = net_config(spki_hash);
    // since we're using replicon, we map each replicon channel to a lane, 1 to 1
    let session_config = SessionConfig::default()
        .with_send_lanes(channels.client_channels())
        .with_recv_lanes(channels.server_channels());

    if let Err(err) = client.connect(runtime.as_ref(), net_config, session_config, target) {
        warn!("Failed to start connecting: {:#}", pretty_error(&err));
    }
}

#[cfg(target_family = "wasm")]
fn net_config(cert_hash: String) -> ClientConfig {
    use aeronet_webtransport::xwt_web_sys::{CertificateHash, HashAlgorithm, WebTransportOptions};

    let server_certificate_hashes = match cert::hash_from_b64(&cert_hash) {
        Ok(hash) => vec![CertificateHash {
            algorithm: HashAlgorithm::Sha256,
            value: Vec::from(hash),
        }],
        Err(err) => {
            warn!(
                "Failed to read certificate hash from string: {:#}",
                pretty_error(&err)
            );
            Vec::new()
        }
    };

    WebTransportOptions {
        server_certificate_hashes,
        ..Default::default()
    }
}

#[cfg(not(target_family = "wasm"))]
fn net_config(cert_hash: String) -> ClientConfig {
    use aeronet_webtransport::wtransport::tls::Sha256Digest;
    use web_time::Duration;

    let config = ClientConfig::builder().with_bind_default();

    let config = if cert_hash.is_empty() {
        info!("*** Connecting with no certificate validation! ***");
        config.with_no_cert_validation()
    } else {
        match cert::hash_from_b64(&cert_hash) {
            Ok(hash) => config.with_server_certificate_hashes([Sha256Digest::new(hash)]),
            Err(err) => {
                warn!(
                    "Failed to read certificate hash from string: {:#}",
                    pretty_error(&err)
                );
                config.with_server_certificate_hashes([])
            }
        }
    };

    config
        .keep_alive_interval(Some(Duration::from_secs(1)))
        .max_idle_timeout(Some(Duration::from_secs(5)))
        .unwrap()
        .build()
}

fn ui(
    mut commands: Commands,
    mut egui: EguiContexts,
    mut ui_state: ResMut<UiState>,
    mut client: ResMut<WebTransportClient>,
    connect_to_remote: Res<ConnectToRemote>,
) {
    egui::Window::new("Client").show(egui.ctx_mut(), |ui| {
        let pressed_enter = ui.input(|i| i.key_pressed(egui::Key::Enter));

        let mut do_connect = false;
        let mut do_disconnect = false;
        ui.horizontal(|ui| {
            let target_resp = ui.add_enabled(
                client.state().is_disconnected(),
                egui::TextEdit::singleline(&mut ui_state.target).hint_text("https://[::1]:25565"),
            );

            if client.state().is_disconnected() {
                do_connect |= target_resp.lost_focus() && pressed_enter;
                do_connect |= ui.button("Connect").clicked();
            } else {
                do_disconnect |= ui.button("Disconnect").clicked();
            }
        });

        ui.horizontal(|ui| {
            let cert_hash_resp = ui.add_enabled(
                client.state().is_disconnected(),
                egui::TextEdit::singleline(&mut ui_state.cert_hash)
                    .hint_text("Certificate hash (optional)"),
            );

            if client.state().is_disconnected() {
                do_connect |= cert_hash_resp.lost_focus() && pressed_enter;
            }
        });

        if do_connect {
            let target = ui_state.target.clone();
            let cert_hash = ui_state.cert_hash.clone();
            commands.run_system_with_input(**connect_to_remote, (target, cert_hash));
        }

        if do_disconnect {
            let _ = client.disconnect("pressed disconnect button");
        }
    });
}

fn on_connected(
    mut events: EventReader<LocalClientConnected<WebTransportClient>>,
    mut game_state: ResMut<NextState<GameState>>,
) {
    for LocalClientConnected { .. } in events.read() {
        info!("Client connected");
        game_state.set(GameState::Playing);
    }
}

fn on_disconnected(
    mut events: EventReader<LocalClientDisconnected<WebTransportClient>>,
    mut game_state: ResMut<NextState<GameState>>,
) {
    for LocalClientDisconnected { reason } in events.read() {
        info!("Client disconnected: {:#}", pretty_error(&reason));
        game_state.set(GameState::None);
    }
}

fn init_player(
    mut commands: Commands,
    query: Query<Entity, (With<Player>, Without<StateScoped<GameState>>)>,
) {
    for entity in &query {
        commands
            .entity(entity)
            .insert(StateScoped(GameState::Playing));
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
        stats_visualizer.draw(egui.ctx_mut(), client.session(), &stats);
    }
}
