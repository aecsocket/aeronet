//!

use std::{convert::Infallible, mem, string::FromUtf8Error, time::Duration};

use aeronet::{
    AsyncRuntime, ChannelKey, FromServer, LocalClientConnected, LocalClientDisconnected, OnChannel,
    TransportClient, TransportClientPlugin, TransportProtocol, TryFromBytes, TryIntoBytes,
};
use aeronet_wt_native::{ClientState, WebTransportClient, WebTransportProtocol};
use anyhow::Result;
use bevy::{log::LogPlugin, prelude::*};
use bevy_egui::{
    egui::{self, Color32},
    EguiContexts, EguiPlugin,
};
use wtransport::ClientConfig;

// protocol

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ChannelKey)]
#[channel_kind(Unreliable)]
struct AppChannel;

#[derive(Debug, Clone, PartialEq, Eq, Hash, OnChannel)]
#[channel_type(AppChannel)]
#[on_channel(AppChannel)]
struct AppMessage(String);

impl<T> From<T> for AppMessage
where
    T: Into<String>,
{
    fn from(value: T) -> Self {
        Self(value.into())
    }
}

impl TryIntoBytes for AppMessage {
    type Output<'a> = &'a [u8];

    type Error = Infallible;

    fn try_into_bytes(&self) -> Result<Self::Output<'_>, Self::Error> {
        Ok(self.0.as_bytes())
    }
}

impl TryFromBytes for AppMessage {
    type Error = FromUtf8Error;

    fn try_from_bytes(buf: &[u8]) -> Result<Self, Self::Error> {
        String::from_utf8(buf.to_owned().into_iter().collect()).map(AppMessage)
    }
}

struct AppProtocol;

impl TransportProtocol for AppProtocol {
    type C2S = AppMessage;
    type S2C = AppMessage;
}

impl WebTransportProtocol for AppProtocol {
    type Channel = AppChannel;
}

type Client = WebTransportClient<AppProtocol>;

// resources

#[derive(Debug, Clone)]
struct LogLine {
    color: Color32,
    msg: String,
}

impl LogLine {
    fn connect_request(url: impl AsRef<str>) -> Self {
        let url = url.as_ref();
        Self {
            color: Color32::GREEN,
            msg: format!("Connecting to {url}"),
        }
    }

    fn connected() -> Self {
        Self {
            color: Color32::WHITE,
            msg: format!("Connected"),
        }
    }

    fn recv(msg: impl AsRef<str>) -> Self {
        let msg = msg.as_ref();
        Self {
            color: Color32::GRAY,
            msg: format!("> {msg}"),
        }
    }

    fn send(msg: impl AsRef<str>) -> Self {
        let msg = msg.as_ref();
        Self {
            color: Color32::GRAY,
            msg: format!("< {msg}"),
        }
    }

    fn disconnected<E>(err: &E) -> Self
    where
        E: std::error::Error,
    {
        Self {
            color: Color32::RED,
            msg: format!("Disconnected: {:#}", aeronet::error::as_pretty(err)),
        }
    }
}

#[derive(Debug, Default, Resource)]
struct ClientUiState {
    log: Vec<LogLine>,
    url: String,
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
            TransportClientPlugin::<_, Client>::default(),
        ))
        .init_resource::<AsyncRuntime>()
        .init_resource::<Client>()
        .init_resource::<ClientUiState>()
        .add_systems(Update, (update, ui).chain())
        .run();
}

fn client_config() -> ClientConfig {
    ClientConfig::builder()
        .with_bind_default()
        .with_no_cert_validation()
        .keep_alive_interval(Some(Duration::from_secs(5)))
        .build()
}

fn update(
    mut ui_state: ResMut<ClientUiState>,
    mut connected: EventReader<LocalClientConnected>,
    mut recv: EventReader<FromServer<AppProtocol>>,
    mut disconnected: EventReader<LocalClientDisconnected<AppProtocol, Client>>,
) {
    for LocalClientConnected in connected.read() {
        ui_state.log.push(LogLine::connected());
    }

    for FromServer { msg } in recv.read() {
        ui_state.log.push(LogLine::recv(&msg.0));
    }

    for LocalClientDisconnected { cause } in disconnected.read() {
        ui_state.log.push(LogLine::disconnected(cause));
    }
}

fn ui(
    rt: Res<AsyncRuntime>,
    mut egui: EguiContexts,
    mut client: ResMut<Client>,
    mut ui_state: ResMut<ClientUiState>,
) {
    egui::CentralPanel::default().show(egui.ctx_mut(), |ui| {
        scrollback(ui, &ui_state.log);

        if client.state() == ClientState::Disconnected {
            let url_resp = ui
                .horizontal(|ui| {
                    ui.label("URL");
                    ui.add(
                        egui::TextEdit::singleline(&mut ui_state.url)
                            .hint_text("https://[::1]:25565 | [enter] to connect"),
                    )
                })
                .inner;

            if url_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let url = mem::take(&mut ui_state.url).trim().to_string();
                ui_state.log.push(LogLine::connect_request(&url));

                let backend = client
                    .connect(client_config(), url)
                    .expect("backend should be disconnected");
                rt.0.spawn(backend);
            }
        } else {
            let buf_resp = ui
                .horizontal(|ui| {
                    ui.label("Message");
                    ui.add(
                        egui::TextEdit::singleline(&mut ui_state.buf).hint_text("[enter] to send"),
                    )
                })
                .inner;

            if buf_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let buf = mem::take(&mut ui_state.buf);
                if !buf.is_empty() {
                    ui_state.log.push(LogLine::send(&buf));
                    let _ = client.send(buf);

                    ui.memory_mut(|m| m.request_focus(buf_resp.id));
                }
            }

            if ui.button("Disconnect").clicked() {
                let _ = client.disconnect();
            }
        }
    });
}

fn scrollback(ui: &mut egui::Ui, scrollback: &[LogLine]) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        for line in scrollback {
            ui.label(
                egui::RichText::new(&line.msg)
                    .font(egui::FontId::monospace(14.0))
                    .color(line.color),
            );
        }
    });
}
