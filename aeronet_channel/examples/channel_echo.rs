//!

use aeronet::{FromClient, ToClient, ToServer, TransportClientPlugin, TransportServerPlugin};
use aeronet_channel::{ChannelClient, ChannelServer};
use aeronet_example::{
    client_log, log_lines, msg_buf, server_log, EchoMessage, EchoProtocol, Log, LogLine,
};
use bevy::{log::LogPlugin, prelude::*};
use bevy_egui::{egui, EguiContexts, EguiPlugin};

type Client = ChannelClient<EchoProtocol>;
type Server = ChannelServer<EchoProtocol>;

#[derive(Debug, Default, Resource)]
struct ClientUiState {
    log: Vec<LogLine>,
    buf: String,
}

impl Log for ClientUiState {
    fn lines(&mut self) -> &mut Vec<LogLine> {
        &mut self.log
    }
}

#[derive(Debug, Default, Resource)]
struct ServerUiState {
    log: Vec<LogLine>,
}

impl Log for ServerUiState {
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
            TransportServerPlugin::<_, Server>::default(),
        ))
        .init_resource::<ClientUiState>()
        .init_resource::<ServerUiState>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                (client_log::<_, Client, ClientUiState>, client_ui).chain(),
                (
                    server_reply,
                    server_log::<_, Server, ServerUiState>,
                    server_ui,
                )
                    .chain(),
            ),
        )
        .run();
}

fn setup(mut commands: Commands) {
    let mut server = Server::new();
    let (client, _) = Client::connected(&mut server);

    commands.insert_resource(server);
    commands.insert_resource(client);
}

fn client_ui(
    mut egui: EguiContexts,
    mut ui_state: ResMut<ClientUiState>,
    mut send: EventWriter<ToServer<EchoProtocol>>,
) {
    egui::Window::new("Client").show(egui.ctx_mut(), |ui| {
        log_lines(ui, &ui_state.log);

        if let Some(msg) = msg_buf(ui, &mut ui_state.buf) {
            send.send(ToServer { msg });
        }
    });
}

fn server_reply(
    mut recv: EventReader<FromClient<EchoProtocol, Server>>,
    mut send: EventWriter<ToClient<EchoProtocol, Server>>,
) {
    for FromClient { client, msg } in recv.read() {
        let msg = format!("You sent: {}", msg.0);
        send.send(ToClient {
            client: *client,
            msg: EchoMessage(msg),
        });
    }
}

fn server_ui(mut egui: EguiContexts, ui_state: Res<ServerUiState>) {
    egui::Window::new("Server").show(egui.ctx_mut(), |ui| {
        log_lines(ui, &ui_state.log);
    });
}
