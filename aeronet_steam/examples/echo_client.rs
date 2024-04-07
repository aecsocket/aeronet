use std::{convert::Infallible, net::SocketAddr, string::FromUtf8Error};

use aeronet::{
    bevy_tokio_rt::TokioRuntime,
    bytes::Bytes,
    client::{
        ClientState, ClientTransport, ClientTransportPlugin, FromServer, LocalClientConnected,
        LocalClientDisconnected,
    },
    error::pretty_error,
    lane::{LaneKey, OnLane},
    message::{Message, TryFromBytes, TryIntoBytes},
    protocol::{ProtocolVersion, TransportProtocol},
};
use aeronet_steam::client::{ConnectTarget, SteamClientConfig, SteamClientTransport};
use bevy::{log::LogPlugin, prelude::*};
use bevy_ecs::system::SystemId;
use bevy_egui::{egui, EguiContexts, EguiPlugin};

// protocol

// Defines what kind of lanes are available to transport messages over on this
// app's protocol.
//
// This can also be an enum, with each variant representing a different lane,
// and each lane having different guarantees.
#[derive(Debug, Clone, Copy, LaneKey)]
#[lane_kind(UnreliableSequenced)]
struct AppLane;

// Type of message that is transported between clients and servers.
// This is up to you, the user, to define. You can have different types
// for client-to-server and server-to-client transport.
#[derive(Debug, Clone, Message, OnLane)]
#[on_lane(AppLane)]
struct AppMessage(String);

impl<T: Into<String>> From<T> for AppMessage {
    fn from(value: T) -> Self {
        Self(value.into())
    }
}

// Defines how this message type can be converted to/from a Bytes form.
impl TryIntoBytes for AppMessage {
    type Error = Infallible;

    fn try_into_bytes(self) -> Result<Bytes, Self::Error> {
        Ok(Bytes::from(self.0))
    }
}

impl TryFromBytes for AppMessage {
    type Error = FromUtf8Error;

    fn try_from_bytes(buf: Bytes) -> Result<Self, Self::Error> {
        String::from_utf8(Vec::from(buf)).map(AppMessage)
    }
}

// Combines everything above into a single "protocol" type,
// used in your transport
struct AppProtocol;

impl TransportProtocol for AppProtocol {
    type C2S = AppMessage;
    type S2C = AppMessage;
}

const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion(0xdeadbeefbadc0de);

type Client = SteamClientTransport<AppProtocol>;

// logic

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins.set(LogPlugin {
                filter: "wgpu=error,naga=warn,aeronet=debug".into(),
                ..default()
            }),
            EguiPlugin,
            ClientTransportPlugin::<_, Client>::default(),
        ))
        .init_resource::<TokioRuntime>()
        .init_resource::<Client>()
        .init_resource::<UiState>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (update_steam, on_connected, on_disconnected, on_recv, ui).chain(),
        )
        .run();
}

#[derive(Clone, Resource, Deref, DerefMut)]
struct SteamClient(steamworks::Client);

// Register a one-shot system for connecting the client
#[derive(Debug, Clone, Resource, Deref, DerefMut)]
struct ConnectSystem(SystemId<SocketAddr>);

fn setup(world: &mut World) {
    let (steam, steam_single) = steamworks::Client::init_app(480).unwrap();
    steam.networking_utils().init_relay_network_access();
    world.insert_resource(SteamClient(steam));
    world.insert_non_send_resource(steam_single);

    let client = SteamClientTransport::<AppProtocol>::disconnected();
    world.insert_resource(client);

    let connect_system = world.register_system(connect);
    world.insert_resource(ConnectSystem(connect_system));
}

fn update_steam(steam: NonSend<steamworks::SingleClient>) {
    steam.run_callbacks();
}

fn connect(
    In(target): In<SocketAddr>,
    steam: Res<SteamClient>,
    mut client: ResMut<Client>,
    tokio: Res<TokioRuntime>,
    mut ui_state: ResMut<UiState>,
) {
    match client.connect(
        (**steam).clone(),
        ConnectTarget::Ip(target),
        SteamClientConfig::new(PROTOCOL_VERSION, AppLane::ALL),
    ) {
        Ok(backend) => {
            tokio.spawn(backend);
        }
        Err(err) => ui_state.log.push(format!(
            "Failed to connect to {target:?}: {:#}",
            pretty_error(&err)
        )),
    }
}

#[derive(Debug, Default, Resource)]
struct UiState {
    ip: String,
    log: Vec<String>,
    msg: String,
}

fn on_connected(
    mut events: EventReader<LocalClientConnected<AppProtocol, Client>>,
    mut ui_state: ResMut<UiState>,
) {
    for LocalClientConnected { .. } in events.read() {
        ui_state.log.push(format!("Connected"));
    }
}

fn on_disconnected(
    mut events: EventReader<LocalClientDisconnected<AppProtocol, Client>>,
    mut ui_state: ResMut<UiState>,
) {
    for LocalClientDisconnected { reason } in events.read() {
        ui_state
            .log
            .push(format!("Disconnected: {:#}", pretty_error(&reason)));
    }
}

fn on_recv(
    mut events: EventReader<FromServer<AppProtocol, Client>>,
    mut ui_state: ResMut<UiState>,
) {
    for FromServer { msg, .. } in events.read() {
        ui_state.log.push(format!("> {}", msg.0));
    }
}

fn ui(
    mut commands: Commands,
    connect_system: Res<ConnectSystem>,
    mut egui: EguiContexts,
    mut client: ResMut<Client>,
    mut ui_state: ResMut<UiState>,
) {
    egui::CentralPanel::default().show(egui.ctx_mut(), |ui| {
        ui.horizontal(|ui| {
            ui.add_enabled_ui(client.state().is_disconnected(), |ui| {
                let ip = ui
                    .horizontal(|ui| {
                        ui.label("IP");
                        text_input(ui, &mut ui_state.ip)
                    })
                    .inner;
                match ip.map(|ip| ip.parse::<SocketAddr>()) {
                    Some(Ok(ip)) => {
                        ui_state.log.push(format!("Connecting to {ip}"));
                        commands.run_system_with_input(**connect_system, ip);
                    }
                    Some(Err(err)) => {
                        ui_state.log.push(format!("Failed to parse IP: {err:#}"));
                    }
                    _ => {}
                }
            });

            ui.add_enabled_ui(!client.state().is_disconnected(), |ui| {
                if ui.button("Disconnect").clicked() {
                    ui_state.log.push(format!("Disconnected by user"));
                    client
                        .disconnect()
                        .expect("client should not already be disconnected");
                }
            });
        });

        egui::ScrollArea::vertical().show(ui, |ui| {
            for line in &ui_state.log {
                ui.label(egui::RichText::new(line).font(egui::FontId::monospace(14.0)));
            }
        });

        if let ClientState::Connected(info) = client.state() {
            let msg = ui
                .horizontal(|ui| {
                    ui.label("Send");
                    text_input(ui, &mut ui_state.msg)
                })
                .inner;
            if let Some(msg) = msg {
                ui_state.log.push(format!("< {msg}"));
                client.send(msg).expect("should be able to send message");
            }

            egui::Grid::new("stats").show(ui, |ui| {
                ui.label("RTT");
                ui.label(format!("{:?}", info.rtt));
                ui.end_row();

                // ui.label("Messages sent/received");
                // ui.label(format!("{} sent / {} recv", info.msgs_sent, info.msgs_recv));
                // ui.end_row();

                // ui.label("Message bytes sent/received");
                // ui.label(format!(
                //     "{} sent / {} recv",
                //     info.msg_bytes_sent, info.msg_bytes_recv
                // ));
                // ui.end_row();

                // ui.label("Total bytes sent/received");
                // ui.label(format!(
                //     "{} sent / {} recv",
                //     info.total_bytes_sent, info.total_bytes_recv
                // ));
                // ui.end_row();
            });
        }
    });
}

pub fn text_input(ui: &mut egui::Ui, text: &mut String) -> Option<String> {
    let resp = ui.text_edit_singleline(text);
    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !text.is_empty() {
        ui.memory_mut(|m| m.request_focus(resp.id));
        Some(std::mem::take(text))
    } else {
        None
    }
}
