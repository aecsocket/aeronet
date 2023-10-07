use aeronet::{
    ClientId, ClientTransportPlugin, FromClient, FromServer, LocalClientConnected,
    LocalClientDisconnected, RemoteClientConnected, RemoteClientDisconnected,
    ServerTransportPlugin, ToClient, ToServer, TryFromBytes, TryIntoBytes,
};
use aeronet_channel::{ChannelTransportClient, ChannelTransportServer};
use anyhow::Result;
use bevy::{log::LogPlugin, prelude::*};
use bevy_egui::{egui, EguiContexts, EguiPlugin};

// config

#[derive(Debug, Clone)]
pub struct AppMessage(pub String);

impl TryIntoBytes for AppMessage {
    fn try_into_bytes(self) -> Result<Vec<u8>> {
        Ok(self.0.into_bytes())
    }
}

impl TryFromBytes for AppMessage {
    fn try_from_bytes(payload: &[u8]) -> Result<Self> {
        String::from_utf8(payload.to_owned().into_iter().collect())
            .map(|s| AppMessage(s))
            .map_err(|err| err.into())
    }
}

type Client = ChannelTransportClient<AppMessage, AppMessage>;

type Server = ChannelTransportServer<AppMessage, AppMessage>;

// resources

#[derive(Debug, Clone, Default, Resource)]
struct ClientState {
    scrollback: Vec<String>,
    buf: String,
}

#[derive(Debug, Clone, Default, Resource)]
struct ServerState {
    scrollback: Vec<String>,
    target_client: usize,
    buf: String,
}

// logic

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins.set(LogPlugin {
                level: tracing::Level::DEBUG,
                ..default()
            }),
            EguiPlugin,
            ClientTransportPlugin::<_, _, Client>::default(),
            ServerTransportPlugin::<_, _, Server>::default(),
        ))
        .init_resource::<ClientState>()
        .init_resource::<ServerState>()
        .add_systems(Startup, setup)
        .add_systems(Update, (handle_client, handle_server, client_ui, server_ui).chain())
        .run();
}

fn setup(mut commands: Commands) {
    let mut server = Server::new();
    let client = server.connect();
    commands.insert_resource(server);
    commands.insert_resource(client);
}

fn handle_client(
    mut connected: EventReader<LocalClientConnected>,
    mut disconnected: EventReader<LocalClientDisconnected>,
    mut recv: EventReader<FromServer<AppMessage>>,
    mut state: ResMut<ClientState>,
) {
    for LocalClientConnected in connected.iter() {
        state.scrollback.push("Client connected".into());
    }

    for LocalClientDisconnected { reason } in disconnected.iter() {
        state.scrollback.push(format!(
            "Client disconnected: {:#}",
            aeronet::error::as_pretty(reason),
        ));
    }

    for FromServer { msg } in recv.iter() {
        state.scrollback.push(format!("< {}", msg.0));
    }
}

fn handle_server(
    mut connected: EventReader<RemoteClientConnected>,
    mut disconnected: EventReader<RemoteClientDisconnected>,
    mut recv: EventReader<FromClient<AppMessage>>,
    mut state: ResMut<ServerState>,
) {
    for RemoteClientConnected { client } in connected.iter() {
        state.scrollback.push(format!("Client {client} connected"));
    }

    for RemoteClientDisconnected { client, reason } in disconnected.iter() {
        state.scrollback.push(format!(
            "Client {client} disconnected: {:#}",
            aeronet::error::as_pretty(reason),
        ));
    }

    for FromClient { client, msg } in recv.iter() {
        state.scrollback.push(format!("{client} < {}", msg.0));
    }
}

// ui

fn show_scrollback(ui: &mut egui::Ui, scrollback: &Vec<String>) {
    egui::ScrollArea::vertical()
        .max_height(600.0)
        .show(ui, |ui| {
            for line in scrollback {
                ui.label(egui::RichText::new(line).font(egui::FontId::monospace(14.0)));
            }
        });
}

fn buf_text_edit(ui: &mut egui::Ui, buf: &mut String) -> Option<String> {
    let buf_resp = ui
        .horizontal(|ui| {
            ui.label("Send");
            ui.text_edit_singleline(buf)
        })
        .inner;

    if buf_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        ui.memory_mut(|m| m.request_focus(buf_resp.id));

        let input = buf.clone();
        buf.clear();
        Some(input)
    } else {
        None
    }
}

fn client_ui(
    mut egui: EguiContexts,
    mut state: ResMut<ClientState>,
    mut send: EventWriter<ToServer<AppMessage>>,
) {
    egui::Window::new("Client").show(egui.ctx_mut(), |ui| {
        show_scrollback(ui, &state.scrollback);

        if let Some(buf) = buf_text_edit(ui, &mut state.buf) {
            state.scrollback.push(format!("> {buf}"));
            send.send(ToServer {
                msg: AppMessage(buf),
            });
        }
    });
}

fn server_ui(
    mut egui: EguiContexts,
    mut state: ResMut<ServerState>,
    mut send: EventWriter<ToClient<AppMessage>>,
) {
    egui::Window::new("Server").show(egui.ctx_mut(), |ui| {
        show_scrollback(ui, &state.scrollback);

        let mut target_client_buf = format!("{}", state.target_client);
        ui.horizontal(|ui| {
            ui.label("Target client");
            ui.text_edit_singleline(&mut target_client_buf);
        });
        if let Ok(target_client) = target_client_buf.parse::<usize>() {
            state.target_client = target_client;
        }

        if let Some(buf) = buf_text_edit(ui, &mut state.buf) {
            let target_client = state.target_client;
            state
                .scrollback
                .push(format!("{target_client} > {buf}"));
            send.send(ToClient {
                client: ClientId::from_raw(target_client),
                msg: AppMessage(buf),
            });
        }
    });
}
