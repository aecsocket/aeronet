//! Example showing a Steam sockets client which can send and receive UTF-8
//! strings.

use {
    aeronet_io::{
        Session, SessionEndpoint,
        connection::{Disconnect, Disconnected},
    },
    aeronet_steam::{
        SessionConfig, SteamworksClient,
        client::{SteamNetClient, SteamNetClientPlugin},
    },
    bevy::prelude::*,
    bevy_egui::{EguiContexts, EguiPlugin, egui},
    core::{mem, net::SocketAddr},
    steamworks::{ClientManager, SteamId},
};

fn main() -> AppExit {
    let (steam, steam_single) =
        steamworks::Client::init_app(480).expect("failed to initialize steam");
    steam.networking_utils().init_relay_network_access();

    App::new()
        .insert_resource(SteamworksClient(steam))
        .insert_non_send_resource(steam_single)
        .add_systems(PreUpdate, |steam: NonSend<steamworks::SingleClient>| {
            steam.run_callbacks();
        })
        .add_plugins((
            DefaultPlugins,
            EguiPlugin,
            SteamNetClientPlugin::<ClientManager>::default(),
        ))
        .init_resource::<Log>()
        .add_systems(Update, (global_ui, add_msgs_to_ui, session_ui))
        .add_observer(on_connecting)
        .add_observer(on_connected)
        .add_observer(on_disconnected)
        .run()
}

#[derive(Debug, Default, Deref, DerefMut, Resource)]
struct Log(Vec<String>);

#[derive(Debug, Default, Component)]
struct SessionUi {
    msg: String,
    log: Vec<String>,
}

fn on_connecting(
    trigger: Trigger<OnAdd, SessionEndpoint>,
    names: Query<&Name>,
    mut log: ResMut<Log>,
) {
    let session = trigger.target();
    let name = names
        .get(session)
        .expect("our session entity should have a name");
    log.push(format!("{name} connecting"));
}

fn on_connected(trigger: Trigger<OnAdd, Session>, names: Query<&Name>, mut log: ResMut<Log>) {
    let session = trigger.target();
    let name = names
        .get(session)
        .expect("our session entity should have a name");
    log.push(format!("{name} connected"));
}

fn on_disconnected(trigger: Trigger<Disconnected>, names: Query<&Name>, mut log: ResMut<Log>) {
    let session = trigger.target();
    let name = names
        .get(session)
        .expect("our session entity should have a name");
    log.push(match &*trigger {
        Disconnected::ByUser(reason) => {
            format!("{name} disconnected by user: {reason}")
        }
        Disconnected::ByPeer(reason) => {
            format!("{name} disconnected by peer: {reason}")
        }
        Disconnected::ByError(err) => {
            format!("{name} disconnected due to error: {err:?}")
        }
    });
}

fn global_ui(
    mut egui: EguiContexts,
    mut commands: Commands,
    mut log: ResMut<Log>,
    mut target_addr: Local<String>,
    mut target_peer: Local<String>,
    mut session_id: Local<usize>,
) {
    const DEFAULT_TARGET: &str = "127.0.0.1:25572";

    egui::Window::new("Connect").show(egui.ctx_mut(), |ui| {
        let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));

        let mut connect_addr = false;
        ui.horizontal(|ui| {
            let resp = ui.add(
                egui::TextEdit::singleline(&mut *target_addr)
                    .hint_text(format!("{DEFAULT_TARGET} | [enter] to connect")),
            );
            connect_addr |= resp.lost_focus() && enter_pressed;
            connect_addr |= ui.button("Connect to address").clicked();
        });

        let mut connect_peer = false;
        ui.horizontal(|ui| {
            let resp = ui.add(
                egui::TextEdit::singleline(&mut *target_peer)
                    .hint_text("Steam ID | [enter] to connect"),
            );
            connect_peer |= resp.lost_focus() && enter_pressed;
            connect_peer |= ui.button("Connect to Steam ID").clicked();
        });

        if connect_addr {
            let mut target = target_addr.clone();
            if target.is_empty() {
                DEFAULT_TARGET.clone_into(&mut target);
            }

            match target.parse::<SocketAddr>() {
                Ok(target) => {
                    *session_id += 1;
                    let name = format!("{}. {target}", *session_id);
                    commands
                        .spawn((Name::new(name), SessionUi::default()))
                        .queue(SteamNetClient::<ClientManager>::connect(
                            SessionConfig::default(),
                            target,
                        ));
                }
                Err(err) => {
                    log.push(format!("Invalid address `{target}`: {err:?}"));
                }
            }
        }

        if connect_peer {
            let target = target_peer.clone();

            match target.parse::<u64>() {
                Ok(target) => {
                    let target = SteamId::from_raw(target);
                    *session_id += 1;
                    let name = format!("{}. {target:?}", *session_id);
                    commands
                        .spawn((Name::new(name), SessionUi::default()))
                        .queue(SteamNetClient::<ClientManager>::connect(
                            SessionConfig::default(),
                            target,
                        ));
                }
                Err(err) => {
                    log.push(format!("Invalid Steam ID `{target}`: {err:?}"));
                }
            }
        }

        for msg in log.iter() {
            ui.label(msg);
        }
    });
}

fn add_msgs_to_ui(mut sessions: Query<(&mut SessionUi, &mut Session)>) {
    for (mut ui_state, mut session) in &mut sessions {
        for packet in session.recv.drain(..) {
            let msg =
                String::from_utf8(packet.payload.into()).unwrap_or_else(|_| "(not UTF-8)".into());
            ui_state.log.push(format!("> {msg}"));
        }
    }
}

fn session_ui(
    mut egui: EguiContexts,
    mut commands: Commands,
    mut sessions: Query<(Entity, &Name, &mut SessionUi, Option<&mut Session>)>,
) {
    for (entity, name, mut ui_state, mut session) in &mut sessions {
        egui::Window::new(name.to_string()).show(egui.ctx_mut(), |ui| {
            let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));

            let mut send_msg = false;
            let msg_resp = ui
                .horizontal(|ui| {
                    if session.is_none() {
                        ui.disable();
                    }

                    let msg_resp = ui.add(
                        egui::TextEdit::singleline(&mut ui_state.msg).hint_text("[enter] to send"),
                    );
                    send_msg |= msg_resp.lost_focus() && enter_pressed;
                    send_msg |= ui.button("Send").clicked();
                    msg_resp
                })
                .inner;

            if send_msg {
                if let Some(session) = &mut session {
                    let msg = mem::take(&mut ui_state.msg);
                    ui_state.log.push(format!("< {msg}"));
                    session.send.push(msg.into());
                    ui.memory_mut(|m| m.request_focus(msg_resp.id));
                }
            }

            if ui.button("Disconnect").clicked() {
                commands.trigger_targets(Disconnect::new("pressed disconnect button"), entity);
            }

            let stats = session.as_ref().map(|s| s.stats).unwrap_or_default();

            egui::Grid::new("stats").show(ui, |ui| {
                ui.label("Packet MTU");
                ui.label(format!("{}", session.map(|s| s.mtu()).unwrap_or_default()));
                ui.end_row();

                ui.label("Packets recv/sent");
                ui.label(format!("{} / {}", stats.packets_recv, stats.packets_sent));
                ui.end_row();

                ui.label("Bytes recv/sent");
                ui.label(format!("{} / {}", stats.bytes_recv, stats.bytes_sent));
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
