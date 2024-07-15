//! Example creating a client and server pair, where the client can send a
//! message, and the server just echoes back that message to the client.

use aeronet::{
    client::{ClientEvent, ClientTransport},
    error::pretty_error,
    lane::LaneIndex,
    server::{ServerEvent, ServerTransport},
};
use aeronet_channel::{client::ChannelClient, server::ChannelServer};
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPlugin};
use bytes::Bytes;

// define what lanes our app will use
// we're writing a very simple example, so we'll only use a single lane,
// but your app will probably need more

#[derive(Debug, Clone, Copy)]
struct AppLane;

// implement `Into<LaneIndex>` for your type so that you can pass it directly
// into `send`
impl From<AppLane> for LaneIndex {
    fn from(_: AppLane) -> Self {
        Self::from_raw(0)
    }
}

// some helper structs to display a UI

#[derive(Debug, Default, Resource)]
struct ClientUiState {
    log: Vec<String>,
    msg: String,
}

#[derive(Debug, Default, Resource)]
struct ServerUiState {
    log: Vec<String>,
}

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, EguiPlugin))
        .init_resource::<ClientUiState>()
        .init_resource::<ServerUiState>()
        .add_systems(Startup, setup)
        .add_systems(PreUpdate, (client_poll, server_poll))
        .add_systems(PostUpdate, (client_flush, server_flush))
        .add_systems(Update, (client_ui, server_ui))
        .run();
}

fn setup(mut commands: Commands) {
    let mut server = ChannelServer::open();
    let client = ChannelClient::connect_new(&mut server);
    commands.insert_resource(server);
    commands.insert_resource(client);
}

fn client_poll(
    time: Res<Time>,
    mut client: ResMut<ChannelClient>,
    mut ui_state: ResMut<ClientUiState>,
) {
    for event in client.poll(time.delta()) {
        match event {
            ClientEvent::Connected => {
                ui_state.log.push(format!("Connected"));
            }
            ClientEvent::Disconnected { error } => {
                ui_state
                    .log
                    .push(format!("Disconnected: {:#}", pretty_error(&error)));
            }
            ClientEvent::Recv { msg, .. } => {
                let msg = String::from_utf8(msg.into()).unwrap();
                ui_state.log.push(format!("> {msg}"));
            }
            ClientEvent::Ack { .. } | ClientEvent::Nack { .. } => {}
        }
    }
}

fn client_flush(mut client: ResMut<ChannelClient>) {
    // technically for the channel transport we don't need to flush
    // since messages are guaranteed to be instantly sent along the channel
    // but all other transports must be periodically flushed
    let _ = client.flush();
}

fn client_ui(
    mut egui: EguiContexts,
    mut ui_state: ResMut<ClientUiState>,
    mut client: ResMut<ChannelClient>,
) {
    egui::Window::new("Client").show(egui.ctx_mut(), |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            for line in &ui_state.log {
                ui.label(egui::RichText::new(line).font(egui::FontId::monospace(14.0)));
            }
        });

        ui.add_enabled_ui(client.state().is_connected(), |ui| {
            let (send, msg_resp) = ui
                .horizontal(|ui| {
                    let mut send = false;
                    let msg_resp = ui.text_edit_singleline(&mut ui_state.msg);
                    send |= msg_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    send |= ui.button("Send").clicked();
                    (send, msg_resp)
                })
                .inner;

            if send {
                (|| {
                    ui.memory_mut(|m| m.request_focus(msg_resp.id));

                    let msg = std::mem::take(&mut ui_state.msg);
                    if msg.is_empty() {
                        return;
                    }

                    ui_state.log.push(format!("< {msg}"));
                    let _ = client.send(msg, AppLane);
                })();
            }

            if ui.button("Disconnect").clicked() {
                ui_state.log.push(format!("Disconnected by user"));
                let _ = client.disconnect();
            }
        });
    });
}

fn server_poll(
    time: Res<Time>,
    mut server: ResMut<ChannelServer>,
    mut ui_state: ResMut<ServerUiState>,
) {
    let mut to_send = Vec::new();
    for event in server.poll(time.delta()) {
        match event {
            ServerEvent::Opened => {
                ui_state.log.push(format!("Server opened"));
            }
            ServerEvent::Closed { error } => {
                ui_state
                    .log
                    .push(format!("Server closed: {:#}", pretty_error(&error)));
            }
            ServerEvent::Connecting { client_key } => {
                ui_state.log.push(format!("Client {client_key} connecting"));
            }
            ServerEvent::Connected { client_key } => {
                ui_state.log.push(format!("Client {client_key} connected"));
            }
            ServerEvent::Disconnected { client_key, error } => {
                ui_state.log.push(format!(
                    "Client {client_key} disconnected: {:#}",
                    pretty_error(&error)
                ));
            }
            ServerEvent::Recv {
                client_key, msg, ..
            } => {
                let msg = String::from_utf8(msg.to_vec()).unwrap();
                ui_state.log.push(format!("{client_key} > {}", msg));

                let resp = format!("You sent: {}", msg);
                ui_state.log.push(format!("{client_key} < {resp}"));
                to_send.push((client_key, resp));
            }
            ServerEvent::Ack { .. } | ServerEvent::Nack { .. } => {}
        }
    }

    for (client_key, msg) in to_send {
        let _ = server.send(client_key, Bytes::from(msg), AppLane);
    }
}

fn server_flush(mut server: ResMut<ChannelServer>) {
    // technically for the channel transport we don't need to flush
    // since messages are guaranteed to be instantly sent along the channel
    // but all other transports must be periodically flushed
    let _ = server.flush();
}

fn server_ui(mut egui: EguiContexts, ui_state: Res<ServerUiState>) {
    egui::Window::new("Server").show(egui.ctx_mut(), |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            for line in &ui_state.log {
                ui.label(egui::RichText::new(line).font(egui::FontId::monospace(14.0)));
            }
        });
    });
}
