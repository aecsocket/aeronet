use std::mem;

use aeronet::{
    ClientTransport, ClientTransportPlugin, FromClient, FromServer, LocalConnected, Message,
    RemoteConnected, RemoteConnecting, RemoteDisconnected, ServerTransport, ServerTransportPlugin,
    TransportProtocol, ConditionedClient, ConditionedServer, ConditionerConfig,
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

type Client = ConditionedClient<AppProtocol, ChannelClient<AppProtocol>>;

type Server = ConditionedServer<AppProtocol, ChannelServer<AppProtocol>>;

const CONDITIONER_CONFIG: ConditionerConfig = ConditionerConfig {
    loss_rate: 0.25,
    delay_mean: 1.0,
    delay_std_dev: 0.5,
};

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
                (client_update_log, client_ui).chain(),
                (server_update_log, server_ui).chain(),
            ),
        )
        .run();
}

fn setup(mut commands: Commands) {
    let mut server = ConditionedServer::new(ChannelServer::<AppProtocol>::open(), CONDITIONER_CONFIG);
    let client = ConditionedClient::new(ChannelClient::connect_new(&mut server), CONDITIONER_CONFIG);
    commands.insert_resource(server);
    commands.insert_resource(client);
}

fn client_update_log(
    mut ui_state: ResMut<ClientUiState>,
    mut connected: EventReader<LocalConnected<AppProtocol, Client>>,
    mut recv: EventReader<FromServer<AppProtocol>>,
) {
    for LocalConnected { .. } in connected.read() {
        ui_state.log.push(format!("Connected"));
    }

    for FromServer { msg, .. } in recv.read() {
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

                    let msg = mem::take(&mut ui_state.msg);
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

fn server_update_log(
    mut ui_state: ResMut<ServerUiState>,
    mut server: ResMut<Server>,
    mut connecting: EventReader<RemoteConnecting<AppProtocol, Server>>,
    mut connected: EventReader<RemoteConnected<AppProtocol, Server>>,
    mut disconnected: EventReader<RemoteDisconnected<AppProtocol, Server>>,
    mut recv: EventReader<FromClient<AppProtocol>>,
) {
    for RemoteConnecting { client, .. } in connecting.read() {
        ui_state.log.push(format!("Client {client} connecting"));
    }

    for RemoteConnected { client, .. } in connected.read() {
        ui_state.log.push(format!("Client {client} connected"));
    }

    for RemoteDisconnected { client, reason } in disconnected.read() {
        ui_state
            .log
            .push(format!("Client {client} disconnected: {reason:#}"));
    }

    for FromClient { client, msg, .. } in recv.read() {
        ui_state.log.push(format!("{client} > {}", msg.0));

        let resp = format!("You sent: {}", msg.0);
        ui_state.log.push(format!("{client} < {resp}"));
        let _ = server.send(*client, AppMessage(resp));
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
