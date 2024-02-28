use std::{convert::Infallible, net::SocketAddr, string::FromUtf8Error};

use aeronet::{
    client::{
        ClientState, ClientTransport, ClientTransportPlugin, FromServer, LocalClientConnected,
        LocalClientDisconnected,
    },
    LaneKeyOld, Message, OnLane, ProtocolVersion, TransportProtocol, TryAsBytes, TryFromBytes,
};
use aeronet_steam::{ConnectTarget, SteamClientTransport, SteamClientTransportConfig, MTU};
use bevy::{log::LogPlugin, prelude::*};
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
#[lane_type(AppLane)]
#[on_lane(AppLane)]
struct AppMessage(String);

impl<T: Into<String>> From<T> for AppMessage {
    fn from(value: T) -> Self {
        Self(value.into())
    }
}

// Defines how this message type can be converted to/from a [u8] form.
impl TryAsBytes for AppMessage {
    type Output<'a> = &'a [u8];
    type Error = Infallible;

    fn try_as_bytes(&self) -> Result<Self::Output<'_>, Self::Error> {
        Ok(self.0.as_bytes())
    }
}

impl TryFromBytes for AppMessage {
    type Error = FromUtf8Error;

    fn try_from_bytes(buf: &[u8]) -> Result<Self, Self::Error>
    where
        Self: Sized,
    {
        String::from_utf8(buf.to_vec()).map(AppMessage)
    }
}

struct AppProtocol;

impl TransportProtocol for AppProtocol {
    type C2S = AppMessage;
    type S2C = AppMessage;
}

const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion(0xdeadbeefbadc0de);

type Client = SteamClientTransport<AppProtocol>;

// logic

fn main() {
    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins.set(LogPlugin {
            filter: "wgpu=error,naga=warn,aeronet=debug".into(),
            ..default()
        }),
        EguiPlugin,
        ClientTransportPlugin::<_, Client>::default(),
    ))
    .init_resource::<Client>()
    .init_resource::<UiState>()
    .add_systems(Startup, setup)
    .add_systems(
        Update,
        (update_steam, on_connected, on_disconnected, on_recv, ui).chain(),
    );

    #[cfg(not(target_family = "wasm"))]
    app.init_resource::<aeronet::TokioRuntime>();

    app.run();
}

#[derive(Clone, Resource, Deref, DerefMut)]
struct SteamClient(steamworks::Client);

fn setup(world: &mut World) {
    let (steam, steam_single) = steamworks::Client::init_app(480).unwrap();
    steam.networking_utils().init_relay_network_access();
    world.insert_resource(SteamClient(steam));
    world.insert_non_send_resource(steam_single);

    let client = SteamClientTransport::<AppProtocol>::Disconnected;
    world.insert_resource(client);
}

fn update_steam(steam: NonSend<steamworks::SingleClient>) {
    steam.run_callbacks();
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
        ui_state.log.push(format!(
            "Disconnected: {:#}",
            aeronet::util::pretty_error(&reason)
        ));
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
    steam: Res<SteamClient>,
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
                        connect(&steam, client.as_mut(), ip);
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

                ui.label("Messages sent/received");
                ui.label(format!("{} sent / {} recv", info.msgs_sent, info.msgs_recv));
                ui.end_row();

                ui.label("Message bytes sent/received");
                ui.label(format!(
                    "{} sent / {} recv",
                    info.msg_bytes_sent, info.msg_bytes_recv
                ));
                ui.end_row();

                ui.label("Total bytes sent/received");
                ui.label(format!(
                    "{} sent / {} recv",
                    info.total_bytes_sent, info.total_bytes_recv
                ));
                ui.end_row();
            });
        }
    });
}

fn connect(steam: &steamworks::Client, client: &mut Client, target: SocketAddr) {
    client
        .connect(
            steam.clone(),
            SteamClientTransportConfig {
                version: PROTOCOL_VERSION,
                max_packet_len: MTU,
                lanes: AppLane::config(),
                target: ConnectTarget::Ip(target),
            },
        )
        .expect("backend should be disconnected");
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
