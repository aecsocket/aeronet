use aeronet::{
    ClientTransportPlugin, FromServer, LocalClientConnected, LocalClientDisconnected,
    ServerTransportPlugin, ToServer, TryFromBytes, TryIntoBytes,
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
        .init_resource::<UiState>()
        .add_systems(Startup, setup)
        .add_systems(Update, (recv, ui))
        .run();
}

fn setup(mut commands: Commands) {
    let mut server = Server::new();
    let client = server.connect();
    commands.insert_resource(server);
    commands.insert_resource(client);
}

#[derive(Debug, Clone, Default, Resource)]
struct UiState {
    scrollback: Vec<String>,
    buf: String,
}

impl UiState {
    pub fn push(&mut self, text: impl Into<String>) {
        let text = text.into();
        self.scrollback.push(text);
    }
}

fn ui(
    mut egui: EguiContexts,
    mut state: ResMut<UiState>,
    mut send: EventWriter<ToServer<AppMessage>>,
) {
    egui::Window::new("Client").show(egui.ctx_mut(), |ui| {
        egui::ScrollArea::vertical()
            .max_height(600.0)
            .show(ui, |ui| {
                for line in &state.scrollback {
                    ui.label(egui::RichText::new(line).font(egui::FontId::monospace(14.0)));
                }
            });

        let buf_resp = ui
            .horizontal(|ui| {
                ui.label("Send");
                ui.text_edit_singleline(&mut state.buf)
            })
            .inner;

        if buf_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let buf = state.buf.clone();
            state.buf.clear();
            state.push(format!("> {}", buf));
            send.send(ToServer {
                msg: AppMessage(buf),
            });

            ui.memory_mut(|m| m.request_focus(buf_resp.id));
        }
    });
}

fn recv(
    mut connected: EventReader<LocalClientConnected>,
    mut disconnected: EventReader<LocalClientDisconnected>,
    mut recv: EventReader<FromServer<AppMessage>>,
    mut state: ResMut<UiState>,
) {
    for LocalClientConnected in connected.iter() {
        state.push("Client connected");
    }

    for LocalClientDisconnected { reason } in disconnected.iter() {
        state.push(format!(
            "Client disconnected: {:#}",
            aeronet::error::as_pretty(reason),
        ));
    }

    for FromServer { msg } in recv.iter() {
        state.push(format!("< {}", msg.0));
    }
}
