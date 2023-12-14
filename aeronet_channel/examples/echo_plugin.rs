//!

use std::{convert::Infallible, mem, string::FromUtf8Error};

use aeronet::{
    FromClient, FromServer, LocalClientConnected, LocalClientDisconnected, RemoteClientConnected,
    RemoteClientDisconnected, ToClient, ToServer, TransportClientPlugin, TransportProtocol,
    TransportServerPlugin, TryFromBytes, TryIntoBytes,
};
use bevy::{log::LogPlugin, prelude::*};
use bevy_egui::{egui, EguiContexts, EguiPlugin};

// protocol

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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

type Client = aeronet_channel::ChannelClient<AppProtocol>;
type Server = aeronet_channel::ChannelServer<AppProtocol>;

// resources

#[derive(Debug, Default, Resource)]
struct ClientState {
    scrollback: Vec<String>,
    buf: String,
}

#[derive(Debug, Default, Resource)]
struct ServerState {
    scrollback: Vec<String>,
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
            TransportServerPlugin::<_, Server>::default(),
            TransportClientPlugin::<_, Client>::default(),
        ))
        .init_resource::<ServerState>()
        .init_resource::<ClientState>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (update_client, client_ui, update_server, server_ui).chain(),
        )
        .run();
}

fn setup(mut commands: Commands) {
    let mut server = Server::new();
    let (client, _) = Client::connected(&mut server);

    commands.insert_resource(server);
    commands.insert_resource(client);
}

fn show_scrollback(ui: &mut egui::Ui, scrollback: &[String]) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        for line in scrollback {
            ui.label(egui::RichText::new(line).font(egui::FontId::monospace(14.0)));
        }
    });
}

fn update_client(
    mut state: ResMut<ClientState>,
    mut connected: EventReader<LocalClientConnected>,
    mut recv: EventReader<FromServer<AppProtocol>>,
    mut disconnected: EventReader<LocalClientDisconnected<AppProtocol, Client>>,
    mut send: EventReader<ToServer<AppProtocol>>,
) {
    for LocalClientConnected in connected.read() {
        state.scrollback.push(format!("Connected"));
    }

    for FromServer { msg } in recv.read() {
        state.scrollback.push(format!("> {}", msg.0));
    }

    for ToServer { msg } in send.read() {
        state.scrollback.push(format!("< {}", msg.0));
    }

    for LocalClientDisconnected { cause } in disconnected.read() {
        state.scrollback.push(format!(
            "Disconnected: {:#}",
            aeronet::error::as_pretty(&cause),
        ));
    }
}

fn client_ui(
    mut egui: EguiContexts,
    mut state: ResMut<ClientState>,
    mut send: EventWriter<ToServer<AppProtocol>>,
) {
    egui::Window::new("Client").show(egui.ctx_mut(), |ui| {
        show_scrollback(ui, state.scrollback.as_slice());

        let buf_resp = ui.text_edit_singleline(&mut state.buf);
        if buf_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let buf = mem::take(&mut state.buf);
            if buf.is_empty() {
                return;
            }

            send.send(ToServer {
                msg: AppMessage(buf),
            });

            ui.memory_mut(|m| m.request_focus(buf_resp.id));
        }
    });
}

fn update_server(
    mut state: ResMut<ServerState>,
    mut connected: EventReader<RemoteClientConnected<AppProtocol, Server>>,
    mut recv: EventReader<FromClient<AppProtocol, Server>>,
    mut disconnected: EventReader<RemoteClientDisconnected<AppProtocol, Server>>,
    mut send: EventWriter<ToClient<AppProtocol, Server>>,
) {
    for RemoteClientConnected { client } in connected.read() {
        state.scrollback.push(format!("{client:?} connected"));
    }

    for FromClient { client, msg } in recv.read() {
        state.scrollback.push(format!("{client:?} > {}", msg.0));

        let msg = format!("You sent: {}", msg.0);
        state.scrollback.push(format!("{client:?} < {}", msg));
        send.send(ToClient {
            client: client.clone(),
            msg: AppMessage(msg),
        });
    }

    for RemoteClientDisconnected { client, cause } in disconnected.read() {
        state.scrollback.push(format!(
            "{client:?} disconnected: {:#}",
            aeronet::error::as_pretty(&cause),
        ));
    }
}

fn server_ui(mut egui: EguiContexts, state: Res<ServerState>) {
    egui::Window::new("Server").show(egui.ctx_mut(), |ui| {
        show_scrollback(ui, &state.scrollback);
    });
}
