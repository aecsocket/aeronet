//!

use std::time::Duration;

use aeronet::{ClientState, ToServer, TokioRuntime, TransportClient, TransportClientPlugin};
use aeronet_example::{
    client_log, log_lines, msg_buf, url_buf, EchoMessage, EchoProtocol, Log, LogLine, LOG_FILTER,
};
use aeronet_wt_native::WebTransportClient;
use bevy::{log::LogPlugin, prelude::*};
use bevy_egui::{egui, EguiContexts, EguiPlugin};
use wtransport::ClientConfig;

type Client = WebTransportClient<EchoProtocol>;

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
                filter: LOG_FILTER.to_string(),
                ..default()
            }),
            EguiPlugin,
            TransportClientPlugin::<_, Client>::default(),
        ))
        .init_resource::<TokioRuntime>()
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
    rt: Res<TokioRuntime>,
    mut egui: EguiContexts,
    mut client: ResMut<Client>,
    mut ui_state: ResMut<ClientUiState>,
    mut send: EventWriter<ToServer<EchoProtocol>>,
) {
    egui::CentralPanel::default().show(egui.ctx_mut(), |ui| {
        let can_disconnect = matches!(
            client.client_state(),
            ClientState::Connecting | ClientState::Connected { .. }
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

        if let ClientState::Connected { info } = client.client_state() {
            if let Some(msg) = msg_buf(ui, &mut ui_state.buf) {
                send.send(ToServer {
                    msg: EchoMessage(msg),
                });
            }

            egui::Grid::new("stats").show(ui, |ui| {
                ui.label("RTT");
                ui.label(format!("{:?}", info.rtt));
                ui.end_row();

                ui.label("Bytes sent");
                ui.label(format!("{}", info.bytes_sent));
                ui.end_row();

                ui.label("Bytes received");
                ui.label(format!("{}", info.bytes_recv));
                ui.end_row();
            });
        }
    });
}
