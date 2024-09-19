//! Example showing a session connected over [`aeronet_channel`]'s [`ChannelIo`]
//! IO layer.

use {
    aeronet_channel::{ChannelIo, ChannelIoPlugin},
    aeronet_io::{DisconnectSessionsExt, PacketBuffers},
    bevy::{log::LogPlugin, prelude::*},
    bevy_egui::{egui, EguiContexts, EguiPlugin},
    std::mem,
};

fn main() -> AppExit {
    App::new()
        .add_plugins((
            DefaultPlugins.set(LogPlugin {
                level: tracing::Level::DEBUG,
                ..default()
            }),
            EguiPlugin,
            ChannelIoPlugin,
        ))
        .add_systems(Startup, setup)
        .add_systems(Update, session_ui)
        .run()
}

#[derive(Debug, Default, Component)]
struct SessionUi {
    msg: String,
    log: Vec<String>,
}

fn setup(mut commands: Commands) {
    let a = commands.spawn((Name::new("A"), SessionUi::default())).id();
    let b = commands.spawn((Name::new("B"), SessionUi::default())).id();
    commands.add(ChannelIo::open(a, b));
}

fn session_ui(
    mut egui: EguiContexts,
    mut commands: Commands,
    mut sessions: Query<(Entity, &Name, &mut SessionUi, &mut PacketBuffers)>,
) {
    for (session, name, mut ui_state, mut bufs) in &mut sessions {
        for msg in bufs.drain_recv() {
            let msg = String::from_utf8(msg.into()).unwrap_or_else(|_| "(not UTF-8)".into());
            ui_state.log.push(format!("> {msg}"));
        }

        egui::Window::new(format!("Session {name}")).show(egui.ctx_mut(), |ui| {
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

            egui::ScrollArea::vertical().show(ui, |ui| {
                for msg in &ui_state.log {
                    ui.label(msg);
                }
            });
        });
    }
}
