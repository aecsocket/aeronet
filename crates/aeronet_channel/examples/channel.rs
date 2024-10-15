//! Example showing a session connected over [`aeronet_channel`]'s [`ChannelIo`]
//! IO layer.

use {
    aeronet_channel::{ChannelIo, ChannelIoPlugin},
    aeronet_io::{connection::Disconnect, packet::PacketBuffers},
    bevy::{log::LogPlugin, prelude::*},
    bevy_egui::{egui, EguiContexts, EguiPlugin},
    std::mem,
};

// Standard app setup.

fn main() -> AppExit {
    App::new()
        .add_plugins((
            DefaultPlugins.set(LogPlugin {
                level: tracing::Level::DEBUG,
                ..default()
            }),
            EguiPlugin,
            // Add the IO plugin for the IO layer implementation you want to use.
            // This will automatically add the `AeronetIoPlugin`.
            ChannelIoPlugin,
        ))
        .add_systems(Startup, setup)
        .add_systems(Update, (add_msgs_to_ui, session_ui))
        .run()
}

#[derive(Debug, Default, Component)]
struct SessionUi {
    msg: String,
    log: Vec<String>,
}

fn setup(mut commands: Commands) {
    // Typically, you'll use commands to create a session.
    // With other implementations, you spawn an entity and push an
    // `EntityCommand` onto it to set up the session.
    // This `EntityCommand` is created from info such as the configuration,
    // the URL to connect to, etc.
    // However, `aeronet_channel` is special, and uses a `Command` instead,
    // since it affects two entities at the same time.
    let a = commands.spawn((Name::new("A"), SessionUi::default())).id();
    let b = commands.spawn((Name::new("B"), SessionUi::default())).id();
    commands.add(ChannelIo::open(a, b));
}

fn add_msgs_to_ui(mut sessions: Query<(&mut SessionUi, &mut PacketBuffers)>) {
    for (mut ui_state, mut bufs) in &mut sessions {
        // Use `PacketBuffers` to read and write packets directly.
        // Typically, you'll be using a higher-level feature such as messages.
        for msg in bufs.recv.drain() {
            let msg = String::from_utf8(msg.into()).unwrap_or_else(|_| "(not UTF-8)".into());
            ui_state.log.push(format!("> {msg}"));
        }
    }
}

fn session_ui(
    mut egui: EguiContexts,
    mut commands: Commands,
    mut sessions: Query<(Entity, &Name, &mut SessionUi, &mut PacketBuffers)>,
) {
    for (session, name, mut ui_state, mut bufs) in &mut sessions {
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
                bufs.send.push(msg.into());
                ui.memory_mut(|m| m.request_focus(msg_resp.id));
            }

            if ui.button("Disconnect").clicked() {
                commands.trigger_targets(Disconnect::new("disconnected by user"), session);
            }

            egui::ScrollArea::vertical().show(ui, |ui| {
                for msg in &ui_state.log {
                    ui.label(msg);
                }
            });
        });
    }
}
