//! Example showing a WebSocket client which can send and receive UTF-8
//! strings.

use {
    aeronet_io::{
        connection::{
            Connected, Disconnect, DisconnectReason, Disconnected, LocalAddr, RemoteAddr, Session,
        },
        packet::{PacketBuffers, PacketMtu, PacketStats},
    },
    aeronet_websocket::client::{ClientConfig, WebSocketClient, WebSocketClientPlugin},
    bevy::prelude::*,
    bevy_egui::{egui, EguiContexts, EguiPlugin},
    std::mem,
};

fn main() -> AppExit {
    #[cfg(not(target_family = "wasm"))]
    aeronet_websocket::rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("failed to install default crypto provider");

    App::new()
        .add_plugins((DefaultPlugins, EguiPlugin, WebSocketClientPlugin))
        .init_resource::<GlobalUi>()
        .add_systems(Update, (global_ui, add_msgs_to_ui, session_ui))
        .observe(on_connecting)
        .observe(on_connected)
        .observe(on_disconnected)
        .run()
}

#[derive(Debug, Default, Resource)]
struct GlobalUi {
    target: String,
    session_id: usize,
    log: Vec<String>,
}

#[derive(Debug, Default, Component)]
struct SessionUi {
    msg: String,
    log: Vec<String>,
}

fn on_connecting(
    trigger: Trigger<OnAdd, Session>,
    names: Query<&Name>,
    mut ui_state: ResMut<GlobalUi>,
) {
    let session = trigger.entity();
    let name = names.get(session).unwrap();
    ui_state.log.push(format!("{name} connecting"));
}

fn on_connected(
    trigger: Trigger<OnAdd, Connected>,
    names: Query<&Name>,
    mut ui_state: ResMut<GlobalUi>,
) {
    let session = trigger.entity();
    let name = names.get(session).unwrap();
    ui_state.log.push(format!("{name} connected"));
}

fn on_disconnected(
    trigger: Trigger<Disconnected>,
    names: Query<&Name>,
    mut ui_state: ResMut<GlobalUi>,
) {
    let session = trigger.entity();
    let Disconnected { reason } = trigger.event();
    let name = names.get(session).unwrap();
    ui_state.log.push(match reason {
        DisconnectReason::User(reason) => {
            format!("{name} disconnected by user: {reason}")
        }
        DisconnectReason::Peer(reason) => {
            format!("{name} disconnected by peer: {reason}")
        }
        DisconnectReason::Error(err) => {
            format!("{name} disconnected due to error: {err:#}")
        }
    });
}

const DEFAULT_TARGET: &str = "ws://[::1]:25565";

fn global_ui(mut egui: EguiContexts, mut commands: Commands, mut ui_state: ResMut<GlobalUi>) {
    egui::Window::new("Connect").show(egui.ctx_mut(), |ui| {
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
                target = DEFAULT_TARGET.to_owned();
            }

            let config = client_config();

            ui_state.session_id += 1;
            let name = format!("{}. {target}", ui_state.session_id);
            commands
                .spawn((Name::new(name), SessionUi::default()))
                .add(WebSocketClient::connect(config, target));
        }

        for msg in &ui_state.log {
            ui.label(msg);
        }
    });
}

#[cfg(target_family = "wasm")]
fn client_config() -> ClientConfig {
    ClientConfig::default()
}

#[cfg(not(target_family = "wasm"))]
fn client_config() -> ClientConfig {
    use {
        aeronet_websocket::{rustls, tokio_tungstenite::Connector},
        std::sync::Arc,
    };

    let mut root_certs = rustls::RootCertStore::empty();
    {
        #[cfg(feature = "rustls-tls-native-roots")]
        {
            info!("Using native certificate roots");

            let native_certs = aeronet_websocket::rustls_native_certs::load_native_certs()
                .expect("failed to load platform certs");
            for cert in native_certs {
                root_certs.add(cert).unwrap();
            }
        }

        #[cfg(feature = "rustls-tls-webpki-roots")]
        {
            info!("Using webpki certificate roots");

            root_certs
                .roots
                .extend_from_slice(webpki_roots::TLS_SERVER_ROOTS);
        }
    };

    let rustls_config = Arc::new(
        rustls::ClientConfig::builder()
            .with_root_certificates(root_certs)
            .with_no_client_auth(),
    );

    ClientConfig {
        connector: Connector::Rustls(rustls_config),
        ..Default::default()
    }
}

fn add_msgs_to_ui(mut sessions: Query<(&mut SessionUi, &mut PacketBuffers)>) {
    for (mut ui_state, mut bufs) in &mut sessions {
        for msg in bufs.drain_recv() {
            let msg = String::from_utf8(msg.into()).unwrap_or_else(|_| "(not UTF-8)".into());
            ui_state.log.push(format!("> {msg}"));
        }
    }
}

fn session_ui(
    mut egui: EguiContexts,
    mut commands: Commands,
    mut sessions: Query<(
        Entity,
        &Name,
        &mut SessionUi,
        &mut PacketBuffers,
        Option<&PacketMtu>,
        Option<&PacketStats>,
        Option<&LocalAddr>,
        Option<&RemoteAddr>,
    )>,
) {
    for (
        session,
        name,
        mut ui_state,
        mut bufs,
        packet_mtu,
        packet_stats,
        local_addr,
        remote_addr,
    ) in &mut sessions
    {
        egui::Window::new(name.to_string()).show(egui.ctx_mut(), |ui| {
            let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));

            let mut send_msg = false;
            let msg_resp = ui
                .horizontal(|ui| {
                    let msg_resp = ui.add(
                        egui::TextEdit::singleline(&mut ui_state.msg).hint_text("[enter] to send"),
                    );
                    send_msg |= msg_resp.lost_focus() && enter_pressed;
                    send_msg |= ui.button("Send").clicked();
                    msg_resp
                })
                .inner;

            if send_msg {
                let msg = mem::take(&mut ui_state.msg);
                ui_state.log.push(format!("< {msg}"));
                bufs.push_send(msg.into());
                ui.memory_mut(|m| m.request_focus(msg_resp.id));
            }

            if ui.button("Disconnect").clicked() {
                commands.trigger_targets(Disconnect::new("disconnected by user"), session);
            }

            egui::Grid::new("stats").show(ui, |ui| {
                ui.label("Packet MTU");
                ui.label(
                    packet_mtu
                        .map(|PacketMtu(mtu)| format!("{mtu}"))
                        .unwrap_or_default(),
                );
                ui.end_row();

                ui.label("Packets recv/sent");
                ui.label(
                    packet_stats
                        .map(|stats| format!("{} / {}", stats.packets_recv, stats.packets_sent))
                        .unwrap_or_default(),
                );
                ui.end_row();

                ui.label("Bytes recv/sent");
                ui.label(
                    packet_stats
                        .map(|stats| format!("{} / {}", stats.bytes_recv, stats.bytes_sent))
                        .unwrap_or_default(),
                );
                ui.end_row();

                ui.label("Local address");
                ui.label(
                    local_addr
                        .map(|LocalAddr(addr)| format!("{addr:?}"))
                        .unwrap_or_default(),
                );
                ui.end_row();

                ui.label("Remote address");
                ui.label(
                    remote_addr
                        .map(|RemoteAddr(addr)| format!("{addr:?}"))
                        .unwrap_or_default(),
                );
                ui.end_row();
            });

            egui::ScrollArea::vertical().show(ui, |ui| {
                for msg in &ui_state.log {
                    ui.label(msg);
                }
            });
        });
    }
}
