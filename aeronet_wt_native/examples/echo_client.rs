//!

use std::time::Duration;

use aeronet::{AsyncRuntime, ClientState, ToServer, TransportClient, TransportClientPlugin};
use aeronet_example::{client_log, log_lines, msg_buf, url_buf, AppProtocol, Log, LogLine};
use aeronet_wt_native::WebTransportClient;
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

impl Log for ClientUiState {
    fn lines(&mut self) -> &mut Vec<LogLine> {
        &mut self.log
    }
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
        .add_systems(Update, (client_log::<_, Client, ClientUiState>, ui).chain())
        .run();
}

fn client_config() -> ClientConfig {
    ClientConfig::builder()
        .with_bind_default()
        .with_no_cert_validation()
        .keep_alive_interval(Some(Duration::from_secs(5)))
        .build()
}

fn ui(
    rt: Res<AsyncRuntime>,
    mut egui: EguiContexts,
    mut client: ResMut<Client>,
    mut ui_state: ResMut<ClientUiState>,
    mut send: EventWriter<ToServer<AppProtocol>>,
) {
    egui::CentralPanel::default().show(egui.ctx_mut(), |ui| {
        let can_disconnect = matches!(
            client.state(),
            ClientState::Connecting | ClientState::Connected(_)
        );
        ui.horizontal(|ui| {
            ui.add_enabled_ui(!can_disconnect, |ui| {
                if let Some(url) = url_buf(ui, &mut ui_state.url) {
                    let backend = client
                        .connect(client_config(), url)
                        .expect("backend should be disconnected");
                    rt.0.spawn(backend);
                }
            });

            ui.add_enabled_ui(can_disconnect, |ui| {
                if ui.button("Disconnect").clicked() {
                    let _ = client.disconnect();
                }
            });
        });

        log_lines(ui, &ui_state.log);

        if let ClientState::Connected(info) = client.state() {
            if let Some(msg) = msg_buf(ui, &mut ui_state.buf) {
                send.send(ToServer { msg });
            }

            ui.label(format!("RTT: {:?}", info.rtt));
        }
    });
}
