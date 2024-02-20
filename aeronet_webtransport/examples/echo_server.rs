//!

use std::{convert::Infallible, string::FromUtf8Error, time::Duration};

use aeronet::{
    client::{
        ClientState, ClientTransport, ClientTransportPlugin, FromServer, LocalClientConnected,
        LocalClientDisconnected,
    },
    LaneKey, LaneProtocol, Message, OnLane, ProtocolVersion, TokioRuntime, TransportProtocol,
    TryAsBytes, TryFromBytes,
};
use aeronet_webtransport::{WebTransportClient, WebTransportClientConfig, MTU};
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

impl LaneProtocol for AppProtocol {
    type Lane = AppLane;
}

const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion(0x8080);

type Client = WebTransportClient<AppProtocol>;

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
        .add_systems(Update, (on_connected, on_disconnected, on_recv, ui).chain())
        .run();
}

#[derive(Debug, Default, Resource)]
struct UiState {
    url: String,
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
    runtime: Res<TokioRuntime>,
    mut egui: EguiContexts,
    mut client: ResMut<WebTransportClient<AppProtocol>>,
    mut ui_state: ResMut<UiState>,
) {
    egui::CentralPanel::default().show(egui.ctx_mut(), |ui| {
        ui.horizontal(|ui| {
            ui.add_enabled_ui(client.state().is_disconnected(), |ui| {
                let url = ui
                    .horizontal(|ui| {
                        ui.label("URL");
                        text_input(ui, &mut ui_state.url)
                    })
                    .inner;
                if let Some(url) = url {
                    ui_state.log.push(format!("Connecting to {url}"));
                    connect(runtime.as_ref(), client.as_mut(), url);
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

#[cfg(target_family = "wasm")]
fn native_config() -> aeronet_webtransport::web_sys::WebTransportOptions {
    todo!()
}

#[cfg(not(target_family = "wasm"))]
fn native_config() -> aeronet_webtransport::wtransport::ClientConfig {
    aeronet_webtransport::wtransport::ClientConfig::builder()
        .with_bind_default()
        .with_no_cert_validation()
        .keep_alive_interval(Some(Duration::from_secs(5)))
        .build()
}

fn connect(runtime: &TokioRuntime, client: &mut WebTransportClient<AppProtocol>, url: String) {
    let backend = client
        .connect(WebTransportClientConfig {
            native: native_config(),
            version: PROTOCOL_VERSION,
            max_packet_len: MTU,
            lanes: AppLane::config(),
            url,
        })
        .expect("backend should be disconnected");
    runtime.spawn(backend);
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
