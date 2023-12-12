//!

use std::{convert::Infallible, string::FromUtf8Error};

use aeronet::{
    ClientEvent, ServerEvent, TransportClient, TransportServer, TryFromBytes, TryIntoBytes,
};
use aeronet_channel::{ChannelClient, ChannelServer};
use bevy::{log::LogPlugin, prelude::*};
use bevy_egui::{
    egui::{self, FontId, RichText},
    EguiContexts, EguiPlugin,
};

// config

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct AppMessage(String);

impl<T: Into<String>> From<T> for AppMessage {
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

// resources

type Client = ChannelClient<AppMessage, AppMessage>;
type Server = ChannelServer<AppMessage, AppMessage>;

#[derive(Debug, Resource)]
struct ClientState<const N: usize> {
    client: Client,
    scrollback: Vec<String>,
    buf: String,
}

impl<const N: usize> ClientState<N> {
    fn new(client: Client) -> Self {
        Self {
            client,
            scrollback: Vec::new(),
            buf: String::new(),
        }
    }
}

#[derive(Debug, Resource)]
struct ServerState {
    server: Server,
    scrollback: Vec<String>,
}

impl ServerState {
    fn new(server: Server) -> Self {
        Self {
            server,
            scrollback: Vec::new(),
        }
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
        ))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                update_client::<1>,
                update_client::<2>,
                update_client::<3>,
                update_server,
            ),
        )
        .run();
}

fn setup(mut commands: Commands) {
    let mut server = ChannelServer::new();
    let (client1, _) = ChannelClient::connected(&mut server);
    let (client2, _) = ChannelClient::connected(&mut server);
    let (client3, _) = ChannelClient::connected(&mut server);

    commands.insert_resource(ServerState::new(server));
    commands.insert_resource(ClientState::<1>::new(client1));
    commands.insert_resource(ClientState::<2>::new(client2));
    commands.insert_resource(ClientState::<3>::new(client3));
}

const FONT_ID: FontId = FontId::monospace(14.0);

fn update_client<const N: usize>(mut egui: EguiContexts, mut state: ResMut<ClientState<N>>) {
    let lines = state.client.recv().map(|event| match event {
        ClientEvent::Connected => format!("Connected"),
        ClientEvent::Recv { msg } => format!("< {}", msg.0),
        ClientEvent::Disconnected { cause } => {
            format!("Disconnected: {:#}", aeronet::error::as_pretty(&cause))
        }
    });
    state.scrollback.extend(lines);

    egui::Window::new(format!("Client {}", N)).show(egui.ctx_mut(), |ui| {
        show_scrollback(ui, &state.scrollback);

        let resp = ui.text_edit_singleline(&mut state.buf);
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let buf = state.buf.clone();
            state.buf.clear();
            if buf.is_empty() {
                return;
            }

            match state.client.send(buf.clone()) {
                Ok(_) => state.scrollback.push(format!("> {}", buf)),
                Err(err) => state
                    .scrollback
                    .push(format!("Error: {:#}", aeronet::error::as_pretty(&err))),
            }

            ui.memory_mut(|m| m.request_focus(resp.id));
        }
    });
}

fn update_server(mut egui: EguiContexts, mut state: ResMut<ServerState>) {
    for event in state.server.recv() {
        match event {
            ServerEvent::Connected { client } => {
                state.scrollback.push(format!("{client:?} connected"));
            }
            ServerEvent::Recv { client, msg } => {
                state.scrollback.push(format!("{client:?} < {}", msg.0));
                let msg = format!("You sent: {}", msg.0);
                match state.server.send(client, msg.clone()) {
                    Ok(_) => state.scrollback.push(format!("{client:?} > {msg}")),
                    Err(err) => state.scrollback.push(format!(
                        "Failed to send message: {:#}",
                        aeronet::error::as_pretty(&err)
                    )),
                }
            }
            ServerEvent::Disconnected { client, cause } => {
                state.scrollback.push(format!(
                    "{client:?} disconnected: {:#}",
                    aeronet::error::as_pretty(&cause)
                ));
            }
        }
    }

    egui::Window::new("Server").show(egui.ctx_mut(), |ui| {
        show_scrollback(ui, &state.scrollback);
    });
}

fn show_scrollback(ui: &mut egui::Ui, scrollback: &[String]) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        for line in scrollback {
            ui.label(RichText::new(line).font(FONT_ID));
        }
    });
}
