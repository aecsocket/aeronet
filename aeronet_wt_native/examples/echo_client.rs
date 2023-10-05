use std::time::Duration;

use aeronet::{
    AsyncRuntime, ClientTransportPlugin, FromServer, LocalClientConnected, LocalClientDisconnected,
    ToServer, TryFromBytes, TryIntoBytes,
};
use aeronet_wt_native::{
    wtransport::ClientConfig, ClientStream, StreamMessage, TransportStreams, WebTransportClient, OnStream,
};
use anyhow::Result;
use bevy::{log::LogPlugin, prelude::*};
use bevy_egui::{
    egui::{self, FontId, RichText},
    EguiContexts, EguiPlugin,
};

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

type Client = WebTransportClient<StreamMessage<ClientStream, AppMessage>, AppMessage>;

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
        ))
        .init_resource::<AsyncRuntime>()
        .init_resource::<UiState>()
        .add_systems(Startup, setup)
        .add_systems(Update, ui)
        .add_systems(Update, recv)
        .run();
}

#[derive(Debug, Clone, Resource)]
struct MessageStream(pub ClientStream);

fn setup(mut commands: Commands, rt: Res<AsyncRuntime>) {
    let mut streams = TransportStreams::default();
    let msg_stream = streams.add_bi_c2s();
    commands.insert_resource(MessageStream(msg_stream));

    match create(rt.as_ref(), streams) {
        Ok(client) => {
            commands.insert_resource(client);
            info!("Created client");
        }
        Err(err) => error!("Failed to create client: {err:#}"),
    }
}

fn create(rt: &AsyncRuntime, streams: TransportStreams) -> Result<Client> {
    let config = ClientConfig::builder()
        .with_bind_default()
        .with_no_cert_validation()
        .keep_alive_interval(Some(Duration::from_secs(5)))
        .build();

    let (front, back) = aeronet_wt_native::create_client(config, streams);
    rt.0.spawn(async move {
        back.start().await.unwrap();
    });
    front.connect("https://[::1]:25565");
    Ok(front)
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
    mut send: EventWriter<ToServer<StreamMessage<ClientStream, AppMessage>>>,
    msg_stream: Res<MessageStream>,
) {
    egui::Window::new("Client").show(egui.ctx_mut(), |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            for line in &state.scrollback {
                ui.label(RichText::new(line).font(FontId::monospace(14.0)));
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
                msg: AppMessage(buf).on(msg_stream.0),
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
