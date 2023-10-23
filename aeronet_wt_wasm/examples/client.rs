use aeronet::ClientTransportPlugin;
use aeronet_wt_core::{Channels, OnChannel};
use aeronet_wt_wasm::{WebTransportClient, WebTransportOptions};
use bevy::prelude::*;
use bevy_egui::EguiPlugin;

// config

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Channels)]
#[channel_kind(Datagram)]
struct AppChannel;

#[derive(Debug, Clone, PartialEq, Eq, Hash, OnChannel)]
#[channel_type(AppChannel)]
#[on_channel(AppChannel)]
struct AppMessage(String);

impl TryFromBytes for AppMessage {
    fn try_from_bytes(buf: &[u8]) -> Result<Self> {
        String::from_utf8(buf.to_owned().into_iter().collect())
            .map(AppMessage)
            .map_err(Into::into)
    }
}

impl TryIntoBytes for AppMessage {
    fn try_into_bytes(self) -> Result<Vec<u8>> {
        Ok(self.0.into_bytes())
    }
}

type Client = WebTransportClient<AppMessage, AppMessage, AppChannel>;

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
            ClientTransportPlugin::<_, _, Client>::default(),
        ))
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands) {
    let client = WebTransportClient::new();
    wasm_bindgen_futures::spawn_local(async move {
        client.connect("https://echo.webtransport.day")
            .await
            .unwrap();
    });
}
