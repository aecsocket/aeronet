//! Example showing a Steam sockets client which can send and receive UTF-8
//! strings.

use std::net::SocketAddr;

use aeronet_steam::client::{ConnectTarget, SteamClient};
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPlugin};
use bevy_steamworks::SteamworksPlugin;
use steamworks::{networking_types::NetworkingIdentity, SteamId};

fn main() -> AppExit {
    App::new()
        .add_plugins((
            DefaultPlugins,
            EguiPlugin,
            SteamworksPlugin::init_app(480).unwrap(),
        ))
        .add_systems(Update, (global_ui,))
        .run()
}

#[derive(Debug, Default, Resource)]
struct GlobalUi {
    target_addr: String,
    target_peer: u64,
    session_id: usize,
    log: Vec<String>,
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
            let resp = ui.add(egui::Slider::new(&mut ui_state.target_peer, 0..=u64::MAX));
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
                        .add(SteamClient::connect(target))
                }
                Err(err) => {
                    ui_state
                        .log
                        .push(format!("Invalid address `{}`: {err:?}", target));
                }
            }
        }

        if connect_peer {
            let target = ui_state.target_peer;
            let target = SteamId::from_raw(target);

            ui_state.session_id += 1;
            let name = format!("{}. {target:?}", ui_state.session_id);
            commands
                .spawn((Name::new(name), SessionUi::default()))
                .add(SteamClient::connect(target));
        }

        for msg in &ui_state.log {
            ui.label(msg);
        }
    });
}
