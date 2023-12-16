//!

use std::{convert::Infallible, mem, string::FromUtf8Error};

use aeronet::{
    ChannelKey, ChannelProtocol, OnChannel, TransportProtocol, TryAsBytes, TryFromBytes,
};
use aeronet_wt_wasm::{WebTransportClient, WebTransportConfig};
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPlugin};

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

impl TryAsBytes for AppMessage {
    type Output<'a> = &'a [u8];

    type Error = Infallible;

    fn try_as_bytes(&self) -> Result<Self::Output<'_>, Self::Error> {
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

impl ChannelProtocol for AppProtocol {
    type Channel = AppChannel;
}

type Client = WebTransportClient<AppProtocol>;

// logic

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    fit_canvas_to_parent: true,
                    prevent_default_event_handling: false,
                    ..default()
                }),
                ..default()
            }),
            EguiPlugin,
        ))
        .init_resource::<ClientUiState>()
        .add_systems(Update, ui)
        .run();
}

#[derive(Debug, Default, Resource)]
struct ClientUiState {
    url: String,
}

fn ui(mut egui: EguiContexts, mut ui_state: ResMut<ClientUiState>) {
    egui::CentralPanel::default().show(egui.ctx_mut(), |ui| {
        ui.label("Hello world");

        let url_resp = ui.add(egui::TextEdit::singleline(&mut ui_state.url));
        if url_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            // TODO: trim to [..-1] because egui on WASM adds an "E" to the end
            // TODO: also this code SUCKS
            let url = mem::take(&mut ui_state.url);
            let url = url.trim().to_string();
            let url = url[..url.len() - 1].to_string();
            if !url.is_empty() {
                match Client::connecting(WebTransportConfig::default(), url) {
                    Ok(_) => info!("ok"),
                    Err(err) => warn!("err: {:#}", aeronet::error::as_pretty(&err)),
                }
            }

            ui.memory_mut(|m| m.request_focus(url_resp.id));
        }
    });
}
