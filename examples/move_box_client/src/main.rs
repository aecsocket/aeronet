#![doc = include_str!("../README.md")]

use {
    aeronet::{
        io::{
            Session, SessionEndpoint,
            connection::{Disconnect, DisconnectReason, Disconnected},
        },
        transport::{
            TransportConfig,
            visualizer::{SessionVisualizer, SessionVisualizerPlugin},
        },
    },
    aeronet_replicon::client::{AeronetRepliconClient, AeronetRepliconClientPlugin},
    aeronet_websocket::client::{WebSocketClient, WebSocketClientPlugin},
    aeronet_webtransport::{
        cert,
        client::{WebTransportClient, WebTransportClientPlugin},
    },
    bevy::{ecs::query::QuerySingleError, prelude::*},
    bevy_egui::{EguiContexts, EguiPlugin, egui},
    bevy_replicon::prelude::*,
    move_box::{GameState, MoveBoxPlugin, PlayerColor, PlayerInput, PlayerPosition},
};

fn main() -> AppExit {
    App::new()
        .add_plugins((
            // core
            DefaultPlugins,
            EguiPlugin,
            // transport
            WebTransportClientPlugin,
            WebSocketClientPlugin,
            SessionVisualizerPlugin,
            // replication
            RepliconPlugins,
            AeronetRepliconClientPlugin,
            // game
            MoveBoxPlugin,
        ))
        .init_resource::<GlobalUi>()
        .init_resource::<WebTransportUi>()
        .init_resource::<WebSocketUi>()
        .add_systems(Startup, setup_level)
        .add_systems(
            Update,
            (
                (web_transport_ui, web_socket_ui, global_ui).chain(),
                (draw_boxes, handle_inputs).run_if(in_state(GameState::Playing)),
            ),
        )
        .add_observer(on_connecting)
        .add_observer(on_connected)
        .add_observer(on_disconnected)
        .run()
}

#[derive(Debug, Default, Resource)]
struct GlobalUi {
    session_id: usize,
    log: Vec<String>,
}

#[derive(Debug, Default, Resource)]
struct WebTransportUi {
    target: String,
    cert_hash: String,
}

#[derive(Debug, Default, Resource)]
struct WebSocketUi {
    target: String,
}

fn setup_level(mut commands: Commands) {
    commands.spawn(Camera2d);
}

fn on_connecting(
    trigger: Trigger<OnAdd, SessionEndpoint>,
    names: Query<&Name>,
    mut ui_state: ResMut<GlobalUi>,
) {
    let entity = trigger.entity();
    let name = names
        .get(entity)
        .expect("our session entity should have a name");
    ui_state.log.push(format!("{name} connecting"));
}

fn on_connected(
    trigger: Trigger<OnAdd, Session>,
    names: Query<&Name>,
    mut ui_state: ResMut<GlobalUi>,
    mut game_state: ResMut<NextState<GameState>>,
    mut commands: Commands,
) {
    let entity = trigger.entity();
    let name = names
        .get(entity)
        .expect("our session entity should have a name");
    ui_state.log.push(format!("{name} connected"));

    game_state.set(GameState::Playing);
    commands.entity(entity).insert((
        SessionVisualizer::default(),
        TransportConfig {
            max_memory_usage: 64 * 1024,
            send_bytes_per_sec: 4 * 1024,
            ..default()
        },
    ));
}

fn on_disconnected(
    trigger: Trigger<Disconnected>,
    names: Query<&Name>,
    mut ui_state: ResMut<GlobalUi>,
    mut game_state: ResMut<NextState<GameState>>,
) {
    let session = trigger.entity();
    let name = names
        .get(session)
        .expect("our session entity should have a name");
    ui_state.log.push(match &trigger.reason {
        DisconnectReason::User(reason) => {
            format!("{name} disconnected by user: {reason}")
        }
        DisconnectReason::Peer(reason) => {
            format!("{name} disconnected by peer: {reason}")
        }
        DisconnectReason::Error(err) => {
            format!("{name} disconnected due to error: {err:?}")
        }
    });
    game_state.set(GameState::None);
}

fn global_ui(
    mut commands: Commands,
    mut egui: EguiContexts,
    global_ui: Res<GlobalUi>,
    sessions: Query<(Entity, &Name, Option<&Session>), With<SessionEndpoint>>,
    replicon_client: Res<RepliconClient>,
) {
    let stats = replicon_client.stats();
    egui::Window::new("Session Log").show(egui.ctx_mut(), |ui| {
        ui.label("Replicon reports:");
        ui.horizontal(|ui| {
            ui.label(match replicon_client.status() {
                RepliconClientStatus::Disconnected => "Disconnected",
                RepliconClientStatus::Connecting => "Connecting",
                RepliconClientStatus::Connected { .. } => "Connected",
            });
            ui.separator();

            ui.label(format!("RTT {:.0}ms", stats.rtt * 1000.0));
            ui.separator();

            ui.label(format!("Pkt Loss {:.1}%", stats.packet_loss * 100.0));
            ui.separator();

            ui.label(format!("Rx {:.0}bps", stats.received_bps));
            ui.separator();

            ui.label(format!("Tx {:.0}bps", stats.sent_bps));
        });
        match sessions.get_single() {
            Ok((session, name, connected)) => {
                if connected.is_some() {
                    ui.label(format!("{name} connected"));
                } else {
                    ui.label(format!("{name} connecting"));
                }

                if ui.button("Disconnect").clicked() {
                    commands.trigger_targets(Disconnect::new("disconnected by user"), session);
                }
            }
            Err(QuerySingleError::NoEntities(_)) => {
                ui.label("No sessions active");
            }
            Err(QuerySingleError::MultipleEntities(_)) => {
                ui.label("Multiple sessions active");
            }
        }

        ui.separator();

        for msg in &global_ui.log {
            ui.label(msg);
        }
    });
}

//
// WebTransport
//

fn web_transport_ui(
    mut commands: Commands,
    mut egui: EguiContexts,
    mut global_ui: ResMut<GlobalUi>,
    mut ui_state: ResMut<WebTransportUi>,
    sessions: Query<(), With<Session>>,
) {
    const DEFAULT_TARGET: &str = "https://[::1]:25565";

    egui::Window::new("WebTransport").show(egui.ctx_mut(), |ui| {
        if sessions.iter().next().is_some() {
            ui.disable();
        }

        let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));

        let mut connect = false;
        ui.horizontal(|ui| {
            let connect_resp = ui.add(
                egui::TextEdit::singleline(&mut ui_state.target)
                    .hint_text(format!("{DEFAULT_TARGET} | [enter] to connect")),
            );
            connect |= connect_resp.lost_focus() && enter_pressed;
            connect |= ui.button("Connect").clicked();
        });

        let cert_hash_resp = ui.add(
            egui::TextEdit::singleline(&mut ui_state.cert_hash)
                .hint_text("(optional) certificate hash"),
        );
        connect |= cert_hash_resp.lost_focus() && enter_pressed;

        if connect {
            let mut target = ui_state.target.clone();
            if target.is_empty() {
                DEFAULT_TARGET.clone_into(&mut target);
            }

            let cert_hash = ui_state.cert_hash.clone();
            let config = web_transport_config(cert_hash);

            global_ui.session_id += 1;
            let name = format!("{}. {target}", global_ui.session_id);
            commands
                .spawn((Name::new(name), AeronetRepliconClient))
                .queue(WebTransportClient::connect(config, target));
        }
    });
}

type WebTransportClientConfig = aeronet_webtransport::client::ClientConfig;

#[cfg(target_family = "wasm")]
fn web_transport_config(cert_hash: String) -> WebTransportClientConfig {
    use aeronet_webtransport::xwt_web::{CertificateHash, HashAlgorithm};

    let server_certificate_hashes = match cert::hash_from_b64(&cert_hash) {
        Ok(hash) => vec![CertificateHash {
            algorithm: HashAlgorithm::Sha256,
            value: Vec::from(hash),
        }],
        Err(err) => {
            warn!("Failed to read certificate hash from string: {err:?}");
            Vec::new()
        }
    };

    WebTransportClientConfig {
        server_certificate_hashes,
        ..Default::default()
    }
}

#[cfg(not(target_family = "wasm"))]
fn web_transport_config(cert_hash: String) -> WebTransportClientConfig {
    use {aeronet_webtransport::wtransport::tls::Sha256Digest, core::time::Duration};

    let config = WebTransportClientConfig::builder().with_bind_default();

    let config = if cert_hash.is_empty() {
        warn!("Connecting without certificate validation");
        config.with_no_cert_validation()
    } else {
        match cert::hash_from_b64(&cert_hash) {
            Ok(hash) => config.with_server_certificate_hashes([Sha256Digest::new(hash)]),
            Err(err) => {
                warn!("Failed to read certificate hash from string: {err:?}");
                config.with_server_certificate_hashes([])
            }
        }
    };

    config
        .keep_alive_interval(Some(Duration::from_secs(1)))
        .max_idle_timeout(Some(Duration::from_secs(5)))
        .expect("should be a valid idle timeout")
        .build()
}

//
// WebSocket
//

fn web_socket_ui(
    mut commands: Commands,
    mut egui: EguiContexts,
    mut global_ui: ResMut<GlobalUi>,
    mut ui_state: ResMut<WebSocketUi>,
    sessions: Query<(), With<Session>>,
) {
    const DEFAULT_TARGET: &str = "ws://[::1]:25566";

    egui::Window::new("WebSocket").show(egui.ctx_mut(), |ui| {
        if sessions.iter().next().is_some() {
            ui.disable();
        }

        let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));

        let mut connect = false;
        ui.horizontal(|ui| {
            let connect_resp = ui.add(
                egui::TextEdit::singleline(&mut ui_state.target)
                    .hint_text(format!("{DEFAULT_TARGET} | [enter] to connect")),
            );
            connect |= connect_resp.lost_focus() && enter_pressed;
            connect |= ui.button("Connect").clicked();
        });

        if connect {
            let mut target = ui_state.target.clone();
            if target.is_empty() {
                DEFAULT_TARGET.clone_into(&mut target);
            }

            let config = web_socket_config();

            global_ui.session_id += 1;
            let name = format!("{}. {target}", global_ui.session_id);
            commands
                .spawn((Name::new(name), AeronetRepliconClient))
                .queue(WebSocketClient::connect(config, target));
        }
    });
}

type WebSocketClientConfig = aeronet_websocket::client::ClientConfig;

#[cfg(target_family = "wasm")]
fn web_socket_config() -> WebSocketClientConfig {
    WebSocketClientConfig::default()
}

#[cfg(not(target_family = "wasm"))]
fn web_socket_config() -> WebSocketClientConfig {
    WebSocketClientConfig::builder().with_no_cert_validation()
}

//
// game logic
//

fn handle_inputs(mut inputs: EventWriter<PlayerInput>, input: Res<ButtonInput<KeyCode>>) {
    let mut movement = Vec2::ZERO;
    if input.pressed(KeyCode::ArrowRight) {
        movement.x += 1.0;
    }
    if input.pressed(KeyCode::ArrowLeft) {
        movement.x -= 1.0;
    }
    if input.pressed(KeyCode::ArrowUp) {
        movement.y += 1.0;
    }
    if input.pressed(KeyCode::ArrowDown) {
        movement.y -= 1.0;
    }

    // don't normalize here, since the server will normalize anyway
    inputs.send(PlayerInput { movement });
}

fn draw_boxes(mut gizmos: Gizmos, players: Query<(&PlayerPosition, &PlayerColor)>) {
    for (PlayerPosition(pos), PlayerColor(color)) in &players {
        gizmos.rect_2d(*pos, Vec2::ONE * 50.0, *color);
    }
}
