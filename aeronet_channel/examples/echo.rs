//! Example creating a client and server pair, where the client can send a
//! message, and the server just echoes back that message to the client.

use aeronet::{
    client::{ClientTransport, ClientTransportPlugin, FromServer, LocalClientConnected},
    server::{
        FromClient, RemoteClientConnected, RemoteClientConnecting, RemoteClientDisconnected,
        ServerTransport, ServerTransportPlugin,
    },
    Message, TransportProtocol,
};
use aeronet_channel::{ChannelClient, ChannelServer};
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPlugin};

// Protocol

#[derive(Debug, Clone, Message)]
struct AppMessage(String);

struct AppProtocol;

impl TransportProtocol for AppProtocol {
    type C2S = AppMessage;
    type S2C = AppMessage;
}

type Client = ChannelClient<AppProtocol>;
type Server = ChannelServer<AppProtocol>;

// Logic

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
        .add_plugins((
            DefaultPlugins,
            EguiPlugin,
            ClientTransportPlugin::<_, Client>::default(),
            ServerTransportPlugin::<_, Server>::default(),
        ))
        .init_resource::<ClientUiState>()
        .init_resource::<ServerUiState>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                (client_on_connected, client_on_recv, client_ui).chain(),
                (
                    server_on_connecting,
                    server_on_connected,
                    server_on_recv,
                    server_on_disconnected,
                    server_ui,
                )
                    .chain(),
            ),
        )
        .run();
}

fn setup(mut commands: Commands) {
    let mut server = Server::open();
    let client = Client::connect_new(&mut server);
    commands.insert_resource(server);
    commands.insert_resource(client);
}

fn client_on_connected(
    mut ui_state: ResMut<ClientUiState>,
    mut events: EventReader<LocalClientConnected<AppProtocol, Client>>,
) {
    for LocalClientConnected { .. } in events.read() {
        ui_state.log.push(format!("Connected"));
    }
}

fn client_on_recv(
    mut ui_state: ResMut<ClientUiState>,
    mut events: EventReader<FromServer<AppProtocol, Client>>,
) {
    for FromServer { msg, .. } in events.read() {
        ui_state.log.push(format!("> {}", msg.0));
    }
}

fn client_ui(
    mut egui: EguiContexts,
    mut ui_state: ResMut<ClientUiState>,
    mut client: ResMut<Client>,
) {
    egui::Window::new("Client").show(egui.ctx_mut(), |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            for line in ui_state.log.iter() {
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
                    let _ = client.send(AppMessage(msg));
                })();
            }

            if ui.button("Disconnect").clicked() {
                ui_state.log.push(format!("Disconnected by user"));
                let _ = client.disconnect();
            }
        });
    });
}

fn server_on_connecting(
    mut ui_state: ResMut<ServerUiState>,
    mut events: EventReader<RemoteClientConnecting<AppProtocol, Server>>,
) {
    for RemoteClientConnecting {
        client_key: client, ..
    } in events.read()
    {
        ui_state.log.push(format!("Client {client} connecting"));
    }
}

fn server_on_connected(
    mut ui_state: ResMut<ServerUiState>,
    mut events: EventReader<RemoteClientConnected<AppProtocol, Server>>,
) {
    for RemoteClientConnected {
        client_key: client, ..
    } in events.read()
    {
        ui_state.log.push(format!("Client {client} connected"));
    }
}

fn server_on_recv(
    mut ui_state: ResMut<ServerUiState>,
    mut recv: EventReader<FromClient<AppProtocol, Server>>,
    mut server: ResMut<Server>,
) {
    for FromClient {
        client_key: client,
        msg,
        ..
    } in recv.read()
    {
        ui_state.log.push(format!("{client} > {}", msg.0));

        let resp = format!("You sent: {}", msg.0);
        ui_state.log.push(format!("{client} < {resp}"));
        let _ = server.send(*client, AppMessage(resp));
    }
}

fn server_on_disconnected(
    mut ui_state: ResMut<ServerUiState>,
    mut events: EventReader<RemoteClientDisconnected<AppProtocol, Server>>,
) {
    for RemoteClientDisconnected {
        client_key: client,
        reason,
    } in events.read()
    {
        ui_state
            .log
            .push(format!("Client {client} disconnected: {reason:#}"));
    }
}

fn server_ui(mut egui: EguiContexts, ui_state: Res<ServerUiState>) {
    egui::Window::new("Server").show(egui.ctx_mut(), |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            for line in ui_state.log.iter() {
                ui.label(egui::RichText::new(line).font(egui::FontId::monospace(14.0)));
            }
        });
    });
}
