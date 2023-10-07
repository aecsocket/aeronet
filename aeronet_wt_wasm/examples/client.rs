use aeronet_wt_wasm::{WebTransportClient, WebTransportOptions};
use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins((DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                fit_canvas_to_parent: true,
                prevent_default_event_handling: false,
                ..default()
            }),
            ..default()
        }),))
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands) {
    wasm_bindgen_futures::spawn_local(async move {
        let client = WebTransportClient::new(
            "https://echo.webtransport.day",
            WebTransportOptions::default(),
        )
        .await
        .unwrap();
    });
}
