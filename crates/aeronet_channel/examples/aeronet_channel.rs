//! Example showing a session connected over [`aeronet_channel`]'s [`ChannelIo`]
//! IO layer.

use {
    aeronet_channel::{ChannelIo, ChannelIoPlugin},
    aeronet_io::{DisconnectSessionsExt, PacketBuffers},
    bevy::prelude::*,
    bevy_egui::{egui, EguiContexts, EguiPlugin},
    std::mem,
};

fn main() -> AppExit {
    App::new()
        .add_plugins((DefaultPlugins, EguiPlugin, ChannelIoPlugin))
        .add_systems(Startup, setup)
        .add_systems(Update, ui)
        .run()
}

#[derive(Debug, Default, Component)]
struct UiState {
    msg: String,
    log: Vec<String>,
}

fn setup(world: &mut World) {
    let (a, b) = ChannelIo::from_world(world);
    world.spawn_batch([
        (Name::new("A"), a, UiState::default()),
        (Name::new("B"), b, UiState::default()),
    ]);
}

fn ui(
    mut egui: EguiContexts,
    mut commands: Commands,
    mut sessions: Query<(Entity, &Name, &mut UiState, &mut PacketBuffers)>,
) {
    for (session, name, mut ui_state, mut bufs) in &mut sessions {
        for msg in bufs.drain_recv() {
            let msg = String::from_utf8(Vec::from(msg)).unwrap();
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

            if ui.button("Disconnect").clicked() {
                commands.disconnect_sessions("disconnected by user", session);
            }

            egui::ScrollArea::vertical().show(ui, |ui| {
                for msg in &ui_state.log {
                    ui.label(msg);
                }
            });

            if send_msg {
                let msg = mem::take(&mut ui_state.msg);
                ui_state.log.push(format!("< {msg}"));
                bufs.push_send(msg.into());
                ui.memory_mut(|m| m.request_focus(msg_resp.id));
            }
        });
    }
}
