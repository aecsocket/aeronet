//! Example creating a client and server pair, where the client can send a
//! message, and the server just echoes back that message to the client.

use aeronet::{
    client::{ClientEvent, ClientState, ClientTransport, DisconnectReason},
    error::pretty_error,
    lane::LaneIndex,
    server::{CloseReason, ServerEvent, ServerTransport},
    stats::MessageStats,
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

// implement `Into<LaneIndex>` for your type so that
// you can pass it directly into `send`
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
        .add_systems(
            PreUpdate,
            (
                client_poll,
                server_poll.run_if(resource_exists::<ChannelServer>),
            ),
        )
        .add_systems(
            PostUpdate,
            (
                client_flush,
                server_flush.run_if(resource_exists::<ChannelServer>),
            ),
        )
        .add_systems(Update, (client_ui, server_ui))
        .run();
}

fn setup(mut commands: Commands) {
    let mut server = ChannelServer::new().with_default_disconnect_reason("server dropped");
    server.open().unwrap();

    let mut client = ChannelClient::new();
    client.connect(&mut server).unwrap();
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
            ClientEvent::Disconnected { reason } => {
                ui_state.log.push(match reason {
                    DisconnectReason::Local(reason) | DisconnectReason::Remote(reason) => {
                        format!("Disconnected: {reason}")
                    }
                    DisconnectReason::Error(err) => {
                        format!("Connection error: {:#}", pretty_error(&err))
                    }
                });
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
        let pressed_enter = ui.input(|i| i.key_pressed(egui::Key::Enter));

        let mut do_disconnect = false;
        ui.add_enabled_ui(!client.state().is_disconnected(), |ui| {
            do_disconnect |= ui.button("Disconnected").clicked();
        });

        if do_disconnect {
            ui_state.log.push(format!("Disconnected by user"));
            // Instead of dropping the client, we call `client.disconnect` here
            // to show how your user-defined disconnection reason will be sent to the server.
            // If you had dropped the client instead (e.g. by removing it as a resource),
            // this would have different behavior
            // (see how we handle the server's Close button below...)
            let _ = client.disconnect("disconnected by user");
        }

        let mut do_send = false;
        let msg_resp = ui
            .add_enabled_ui(client.state().is_connected(), |ui| {
                ui.horizontal(|ui| {
                    let msg_resp = ui.add(
                        egui::TextEdit::singleline(&mut ui_state.msg).hint_text("[enter] to send"),
                    );
                    do_send |= msg_resp.lost_focus() && pressed_enter;
                    do_send |= ui.button("Send").clicked();
                    msg_resp
                })
                .inner
            })
            .inner;

        if do_send {
            ui.memory_mut(|m| m.request_focus(msg_resp.id));
            let msg = std::mem::take(&mut ui_state.msg);
            if !msg.is_empty() {
                ui_state.log.push(format!("< {msg}"));
                let _ = client.send(msg, AppLane);
            }
        }

        if let ClientState::Connected(client) = client.state() {
            egui::Grid::new("meta").num_columns(2).show(ui, |ui| {
                ui.label("Bytes sent/recv");
                ui.label(format!("{} / {}", client.bytes_sent(), client.bytes_recv()));
                ui.end_row();
            });
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            for line in &ui_state.log {
                ui.label(egui::RichText::new(line).font(egui::FontId::monospace(14.0)));
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
            ServerEvent::Closed { reason } => {
                ui_state.log.push(match reason {
                    CloseReason::Local(reason) => format!("Server closed: {reason}"),
                    CloseReason::Error(err) => format!("Server error: {:#}", pretty_error(&err)),
                });
            }
            ServerEvent::Connecting { client_key } => {
                ui_state.log.push(format!("Client {client_key} connecting"));
            }
            ServerEvent::Connected { client_key } => {
                ui_state.log.push(format!("Client {client_key} connected"));
            }
            ServerEvent::DisconnectedByError { client_key, error } => {
                ui_state.log.push(format!(
                    "Client {client_key} disconnected due to error: {:#}",
                    pretty_error(&error)
                ));
            }
            ServerEvent::DisconnectedByClient { client_key, reason } => {
                ui_state
                    .log
                    .push(format!("Client {client_key} disconnected: {reason}"));
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

fn server_ui(
    mut commands: Commands,
    mut egui: EguiContexts,
    ui_state: Res<ServerUiState>,
    server: Option<Res<ChannelServer>>,
) {
    egui::Window::new("Server").show(egui.ctx_mut(), |ui| {
        let is_open = server.map_or(false, |server| server.state().is_open());

        let mut do_close = false;
        ui.add_enabled_ui(is_open, |ui| {
            do_close |= ui.button("Close").clicked();
        });

        if do_close {
            // Instead of `server.close`, we'll remove the resource entirely,
            // causing the server to be dropped.
            // This demonstrates how, on drop, the transport will send the default disconnect reason
            // that we set up when we created the server.
            commands.remove_resource::<ChannelServer>();
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            for line in &ui_state.log {
                ui.label(egui::RichText::new(line).font(egui::FontId::monospace(14.0)));
            }
        });
    });
}
