//! Example showing a WebTransport client which can send and receive strings.

use {
    aeronet_io::{
        DisconnectSessionsExt, LocalAddr, PacketBuffers, PacketMtu, PacketRtt, PacketStats,
        RemoteAddr,
    },
    aeronet_webtransport::client::{ClientConfig, WebTransportClient, WebTransportClientPlugin},
    bevy::prelude::*,
    bevy_egui::{egui, EguiContexts, EguiPlugin},
    std::mem,
};

fn main() -> AppExit {
    App::new()
        .add_plugins((DefaultPlugins, EguiPlugin, WebTransportClientPlugin))
        .init_resource::<GlobalUi>()
        .add_systems(Update, (global_ui, session_ui))
        .run()
}

#[derive(Debug, Default, Resource)]
struct GlobalUi {
    target: String,
    session_id: usize,
}

#[derive(Debug, Default, Component)]
struct SessionUi {
    msg: String,
    log: Vec<String>,
}

const DEFAULT_TARGET: &str = "https://[::1]:25565";

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
            let config = client_config();
            let mut target = mem::take(&mut ui_state.target);
            if target.is_empty() {
                target = DEFAULT_TARGET.to_owned();
            }

            ui_state.session_id += 1;
            let name = format!("{}. {target}", ui_state.session_id);
            commands
                .spawn((Name::new(name), SessionUi::default()))
                .add(WebTransportClient::connect(config, target));
        }
    });
}

#[cfg(target_family = "wasm")]
fn client_config() -> ClientConfig {
    ClientConfig::default()
}

#[cfg(not(target_family = "wasm"))]
fn client_config() -> ClientConfig {
    use web_time::Duration;

    ClientConfig::builder()
        .with_bind_default()
        .with_no_cert_validation()
        .keep_alive_interval(Some(Duration::from_secs(1)))
        .max_idle_timeout(Some(Duration::from_secs(5)))
        .unwrap()
        .build()
}

fn session_ui(
    mut egui: EguiContexts,
    mut commands: Commands,
    mut sessions: Query<(
        Entity,
        &Name,
        &mut SessionUi,
        &mut PacketBuffers,
        Option<&PacketRtt>,
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
        packet_rtt,
        packet_mtu,
        packet_stats,
        local_addr,
        remote_addr,
    ) in &mut sessions
    {
        for msg in bufs.drain_recv() {
            let msg = String::from_utf8(msg.into()).unwrap_or_else(|_| "(not UTF-8)".into());
            ui_state.log.push(format!("> {msg}"));
        }

        egui::Window::new(format!("{name}")).show(egui.ctx_mut(), |ui| {
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
                commands.disconnect_sessions("disconnected by user", session);
            }

            egui::Grid::new("stats").show(ui, |ui| {
                let unknown = || "?".to_owned();

                ui.label("Packet RTT");
                ui.label(
                    packet_rtt
                        .map(|PacketRtt(rtt)| format!("{rtt:?}"))
                        .unwrap_or_else(unknown),
                );
                ui.end_row();

                ui.label("Packet MTU");
                ui.label(
                    packet_mtu
                        .map(|PacketMtu(mtu)| format!("{mtu}"))
                        .unwrap_or_else(unknown),
                );
                ui.end_row();

                let stats = packet_stats.copied().unwrap_or_default();

                ui.label("Packets recv/sent");
                ui.label(format!("{} / {}", stats.packets_recv, stats.packets_sent));
                ui.end_row();

                ui.label("Bytes recv/sent");
                ui.label(format!("{} / {}", stats.bytes_recv, stats.bytes_sent));
                ui.end_row();

                ui.label("Local address");
                ui.label(
                    local_addr
                        .map(|LocalAddr(addr)| format!("{addr:?}"))
                        .unwrap_or_else(unknown),
                );
                ui.end_row();

                ui.label("Remote address");
                ui.label(
                    remote_addr
                        .map(|RemoteAddr(addr)| format!("{addr:?}"))
                        .unwrap_or_else(unknown),
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
