//!

use std::{convert::Infallible, mem, string::FromUtf8Error, time::Duration};

use aeronet::{AsyncRuntime, ChannelKey, OnChannel, TryFromBytes, TryIntoBytes, TransportProtocol, TransportClientPlugin, LocalClientConnected, FromServer, LocalClientDisconnected};
use aeronet_wt_native::{WebTransportProtocol, WebTransportClient};
use anyhow::Result;
use bevy::{log::LogPlugin, prelude::*};
use bevy_egui::{egui, EguiContexts, EguiPlugin};
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

#[derive(Debug, Default, Resource)]
struct ClientState {
    scrollback: Vec<String>,
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
        .init_resource::<ClientState>()
        .insert_resource(WebTransportClient::disconnected())
        .add_systems(Update, (update, ui).chain())
        .run();
}

fn create(rt: &AsyncRuntime) -> Result<Client> {
    let config = ClientConfig::builder()
        .with_bind_default()
        .with_no_cert_validation()
        .keep_alive_interval(Some(Duration::from_secs(5)))
        .build();
}

fn update(
    mut state: ResMut<ClientState>,
    mut connected: EventReader<LocalClientConnected>,
    mut recv: EventReader<FromServer<AppProtocol>>,
    mut disconnected: EventReader<LocalClientDisconnected<AppProtocol, Client>>,
) {
    for LocalClientConnected in connected.read() {
        state.scrollback.push(format!("Connected"));
    }

    for FromServer { msg } in recv.read() {
        state.scrollback.push(format!("> {}", msg.0));
    }

    for LocalClientDisconnected { cause } in disconnected.read() {
        state.scrollback.push(format!(
            "Disconnected: {:#}",
            aeronet::error::as_pretty(cause),
        ));
    }
}

fn ui(mut egui: EguiContexts, mut state: ResMut<ClientState>) {
    egui::Window::new("Client").show(egui.ctx_mut(), |ui| {
        show_scrollback(ui, &state.scrollback);

        let buf_resp = ui.text_edit_singleline(&mut state.buf);
        if buf_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let buf = mem::take(&mut state.buf);
            if buf.is_empty() {
                return;
            }
        }
    });
}

fn show_scrollback(ui: &mut egui::Ui, scrollback: &[String]) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        for line in scrollback {
            ui.label(egui::RichText::new(line).font(egui::FontId::monospace(14.0)));
        }
    });
}
