//!

use std::time::Duration;

use aeronet::{
    AsyncRuntime, FromServer, LocalClientConnected, LocalClientDisconnected, TransportClient,
    TransportClientPlugin,
};
use aeronet_example::{log_lines, msg_buf, url_buf, AppProtocol, LogLine};
use aeronet_wt_native::{ClientState, WebTransportClient};
use bevy::{log::LogPlugin, prelude::*};
use bevy_egui::{egui, EguiContexts, EguiPlugin};
use wtransport::ClientConfig;

type Client = WebTransportClient<AppProtocol>;

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
        let connected = client.state() != ClientState::Disconnected;
        ui.horizontal(|ui| {
            ui.add_enabled_ui(!connected, |ui| {
                if let Some(url) = url_buf(ui, &mut ui_state.url) {
                    ui_state.log.push(LogLine::connecting_to(&url));
                    let backend = client
                        .connect(client_config(), url)
                        .expect("backend should be disconnected");
                    rt.0.spawn(backend);
                }
            });

            ui.add_enabled_ui(connected, |ui| {
                if ui.button("Disconnect").clicked() {
                    let _ = client.disconnect();
                }
            });
        });

        log_lines(ui, &ui_state.log);

        if client.state() == ClientState::Connected {
            if let Some(msg) = msg_buf(ui, &mut ui_state.buf) {
                ui_state.log.push(LogLine::send(&msg.0));
                let _ = client.send(msg);
            }
        }
    });
}
