//!

use std::{convert::Infallible, mem, string::FromUtf8Error};

use aeronet::{
    ClientEvent, ServerEvent, TransportClient, TransportProtocol, TransportServer, TryAsBytes,
    TryFromBytes,
};
use aeronet_channel::{ChannelClient, ChannelServer};
use bevy::{log::LogPlugin, prelude::*};

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

#[derive(Debug, Resource)]
struct Client<const N: usize>(ChannelClient<AppProtocol>);

type Server = ChannelServer<AppProtocol>;

// resources

#[derive(Debug, Default, Resource)]
struct ClientState<const N: usize> {
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
        ))
        .init_resource::<ServerState>()
        .init_resource::<ClientState<1>>()
        .init_resource::<ClientState<2>>()
        .init_resource::<ClientState<3>>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                update_client::<1>,
                client_ui::<1>,
                update_client::<2>,
                client_ui::<2>,
                update_client::<3>,
                client_ui::<3>,
                update_server,
                server_ui,
            )
                .chain(),
        )
        .run();
}

fn setup(mut commands: Commands) {
    let mut server = Server::new();
    let (client1, _) = ChannelClient::connected(&mut server);
    let (client2, _) = ChannelClient::connected(&mut server);
    let (client3, _) = ChannelClient::connected(&mut server);

    commands.insert_resource(server);
    commands.insert_resource(Client::<1>(client1));
    commands.insert_resource(Client::<2>(client2));
    commands.insert_resource(Client::<3>(client3));
}

fn update_client<const N: usize>(mut client: ResMut<Client<N>>, mut state: ResMut<ClientState<N>>) {
    for event in client.0.recv() {
        match event {
            ClientEvent::Connected => state.scrollback.push(format!("Connected")),
            ClientEvent::Recv { msg } => state.scrollback.push(format!("> {}", msg.0)),
            ClientEvent::Disconnected { cause } => state.scrollback.push(format!(
                "Disconnected: {:#}",
                aeronet::error::as_pretty(&cause)
            )),
        }
    }
}

fn client_ui<const N: usize>(
    mut egui: EguiContexts,
    mut client: ResMut<Client<N>>,
    mut state: ResMut<ClientState<N>>,
) {
    egui::Window::new(format!("Client {}", N)).show(egui.ctx_mut(), |ui| {
        show_scrollback(ui, &state.scrollback);

        let buf_resp = ui.text_edit_singleline(&mut state.buf);
        if buf_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let buf = mem::take(&mut state.buf);
            if buf.is_empty() {
                return;
            }

            match client.0.send(buf.clone()) {
                Ok(_) => state.scrollback.push(format!("< {}", buf)),
                Err(err) => state
                    .scrollback
                    .push(format!("Error: {:#}", aeronet::error::as_pretty(&err))),
            }

            ui.memory_mut(|m| m.request_focus(buf_resp.id));
        }
    });
}

fn update_server(mut server: ResMut<ChannelServer<AppProtocol>>, mut state: ResMut<ServerState>) {
    for event in server.recv() {
        match event {
            ServerEvent::Connected { client } => {
                state.scrollback.push(format!("{client:?} connected"))
            }
            ServerEvent::Recv { client, msg } => {
                state.scrollback.push(format!("{client:?} > {}", msg.0));

                let msg = format!("You sent: {}", msg.0);
                match server.send(client, msg.clone()) {
                    Ok(_) => state.scrollback.push(format!("{client:?} < {msg}")),
                    Err(err) => state.scrollback.push(format!(
                        "Failed to send message to {client:?}: {:#}",
                        aeronet::error::as_pretty(&err)
                    )),
                }
            }
            ServerEvent::Disconnected { client, cause } => state.scrollback.push(format!(
                "{client:?} disconnected: {:#}",
                aeronet::error::as_pretty(&cause)
            )),
        }
    }
}

fn server_ui(mut egui: EguiContexts, state: Res<ServerState>) {
    egui::Window::new("Server").show(egui.ctx_mut(), |ui| {
        show_scrollback(ui, &state.scrollback);
    });
}

fn show_scrollback(ui: &mut egui::Ui, scrollback: &[String]) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        for line in scrollback {
            ui.label(egui::RichText::new(line).font(egui::FontId::monospace(14.0)));
        }
    });
}
