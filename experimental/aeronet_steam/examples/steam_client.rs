//! Example showing a Steam sockets client which can send and receive UTF-8
//! strings.

use std::{mem, net::SocketAddr};

use aeronet_io::{
    connection::{Connected, Disconnect, DisconnectReason, Disconnected, Session},
    packet::{PacketBuffers, PacketMtu, PacketRtt, PacketStats},
};
use aeronet_steam::{client::SteamClient, config::SteamSessionConfig};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPlugin, egui};
use bevy_steamworks::SteamworksPlugin;
use steamworks::SteamId;

fn main() -> AppExit {
    App::new()
        .add_plugins((
            DefaultPlugins,
            EguiPlugin,
            SteamworksPlugin::init_app(480).unwrap(),
        ))
        .init_resource::<GlobalUi>()
        .add_systems(Update, (global_ui, add_msgs_to_ui, session_ui))
        .observe(on_connecting)
        .observe(on_connected)
        .observe(on_disconnected)
        .run()
}

#[derive(Debug, Default, Resource)]
struct GlobalUi {
    target_addr: String,
    target_peer: String,
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
    let name = names.get(session).unwrap();
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
}

const DEFAULT_TARGET: &str = "127.0.0.1:25565";

fn global_ui(mut egui: EguiContexts, mut commands: Commands, mut ui_state: ResMut<GlobalUi>) {
    egui::Window::new("Connect").show(egui.ctx_mut(), |ui| {
        let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));

        let mut connect_addr = false;
        ui.horizontal(|ui| {
            let resp = ui.add(
                egui::TextEdit::singleline(&mut ui_state.target_addr)
                    .hint_text(format!("{DEFAULT_TARGET} | [enter] to connect")),
            );
            connect_addr |= resp.lost_focus() && enter_pressed;
            connect_addr |= ui.button("Connect to address").clicked();
        });

        let mut connect_peer = false;
        ui.horizontal(|ui| {
            let resp = ui.add(
                egui::TextEdit::singleline(&mut ui_state.target_peer)
                    .hint_text("Steam ID | [enter] to connect"),
            );
            connect_peer |= resp.lost_focus() && enter_pressed;
            connect_peer |= ui.button("Connect to Steam ID").clicked();
        });

        if connect_addr {
            let mut target = ui_state.target_addr.clone();
            if target.is_empty() {
                target = DEFAULT_TARGET.to_owned();
            }

            match target.parse::<SocketAddr>() {
                Ok(target) => {
                    ui_state.session_id += 1;
                    let name = format!("{}. {target}", ui_state.session_id);
                    commands
                        .spawn((Name::new(name), SessionUi::default()))
                        .add(SteamClient::connect(SteamSessionConfig::default(), target));
                }
                Err(err) => {
                    ui_state
                        .log
                        .push(format!("Invalid address `{target}`: {err:?}"));
                }
            }
        }

        if connect_peer {
            let target = ui_state.target_peer.clone();

            match target.parse::<u64>() {
                Ok(target) => {
                    let target = SteamId::from_raw(target);
                    ui_state.session_id += 1;
                    let name = format!("{}. {target:?}", ui_state.session_id);
                    commands
                        .spawn((Name::new(name), SessionUi::default()))
                        .add(SteamClient::connect(SteamSessionConfig::default(), target));
                }
                Err(err) => {
                    ui_state
                        .log
                        .push(format!("Invalid Steam ID `{target}`: {err:?}"));
                }
            }
        }

        for msg in &ui_state.log {
            ui.label(msg);
        }
    });
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
        Option<&PacketRtt>,
        &PacketMtu,
        &PacketStats,
    )>,
) {
    for (session, name, mut ui_state, mut bufs, packet_rtt, packet_mtu, packet_stats) in
        &mut sessions
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
                ui.label("Packet RTT");
                ui.label(
                    packet_rtt
                        .map(|PacketRtt(rtt)| format!("{rtt:?}"))
                        .unwrap_or_default(),
                );
                ui.end_row();

                ui.label("Packet MTU");
                ui.label(format!("{:?}", **packet_mtu));
                ui.end_row();

                ui.label("Packets recv/sent");
                ui.label(format!(
                    "{} / {}",
                    packet_stats.packets_recv, packet_stats.packets_sent
                ));
                ui.end_row();

                ui.label("Bytes recv/sent");
                ui.label(format!(
                    "{} / {}",
                    packet_stats.packets_recv, packet_stats.packets_sent
                ));
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
