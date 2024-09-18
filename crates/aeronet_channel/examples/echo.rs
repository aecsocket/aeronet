//! Example showing a session connected over channel IO, where each session
//! echoes back what the other sends it.

use std::mem;

use aeronet::{
    bytes::Bytes, message::SendMode, session::DisconnectSessionsExt, transport::MessageBuffers,
    AeronetPlugins,
};
use aeronet_channel::{ChannelIo, ChannelIoPlugin};
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPlugin};

fn main() -> AppExit {
    App::new()
        .add_plugins((DefaultPlugins, EguiPlugin, AeronetPlugins, ChannelIoPlugin))
        .add_systems(Startup, setup)
        .add_systems(Update, ui)
        .run()
}

#[derive(Debug, Default, Component)]
struct UiState {
    msg: String,
    log: Vec<String>,
}

fn setup(mut commands: Commands) {
    let (a, b) = ChannelIo::open();
    commands.spawn_batch([
        (Name::new("A"), a, UiState::default()),
        (Name::new("B"), b, UiState::default()),
    ]);
}

fn ui(
    mut egui: EguiContexts,
    mut commands: Commands,
    mut sessions: Query<(Entity, &Name, &mut UiState, &mut MessageBuffers)>,
) {
    for (session, name, mut ui_state, mut bufs) in &mut sessions {
        for msg in bufs.recv.drain(..) {
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
                bufs.send
                    .push((SendMode::UnreliableUnordered, Bytes::from(msg)));
                ui.memory_mut(|m| m.request_focus(msg_resp.id));
            }
        });
    }
}
