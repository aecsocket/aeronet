//!

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPlugin};

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
        .add_systems(Update, ui)
        .run();
}

fn ui(mut egui: EguiContexts) {
    egui::CentralPanel::default().show(egui.ctx_mut(), |ui| {
        ui.label("Hello world");
    });
}
