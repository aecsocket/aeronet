//!

use std::{convert::Infallible, string::FromUtf8Error};

use aeronet::{
    bevy_tokio_rt::TokioRuntime,
    bytes::Bytes,
    client::{
        ClientState, ClientTransport, ClientTransportPlugin, FromServer, LocalClientConnected,
        LocalClientDisconnected,
    },
    lane::{LaneKey, OnLane},
    message::{Message, TryFromBytes, TryIntoBytes},
    protocol::{ProtocolVersion, TransportProtocol},
};
use aeronet_webtransport::client::{WebTransportClient, WebTransportClientConfig};
use bevy::{log::LogPlugin, prelude::*};
use bevy_ecs::system::SystemId;
use bevy_egui::{egui, EguiContexts, EguiPlugin};

//
// protocol
//

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

// Defines how this message type can be converted to/from a [u8] form.
impl TryIntoBytes for AppMessage {
    type Error = Infallible;

    fn try_into_bytes(self) -> Result<Bytes, Self::Error> {
        Ok(Bytes::from(self.0.into_bytes()))
    }
}

impl TryFromBytes for AppMessage {
    type Error = FromUtf8Error;

    fn try_from_bytes(buf: Bytes) -> Result<Self, Self::Error> {
        String::from_utf8(buf.into()).map(AppMessage)
    }
}

struct AppProtocol;

impl TransportProtocol for AppProtocol {
    type C2S = AppMessage;
    type S2C = AppMessage;
}

const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion(0xabcd);

//
// config
//

type Client = WebTransportClient<AppProtocol>;

#[cfg(target_family = "wasm")]
fn native_config() -> aeronet_webtransport::web_sys::WebTransportOptions {
    aeronet_webtransport::web_sys::WebTransportOptions::new()
}

#[cfg(not(target_family = "wasm"))]
fn native_config() -> aeronet_webtransport::wtransport::ClientConfig {
    aeronet_webtransport::wtransport::ClientConfig::builder()
        .with_bind_default()
        .with_no_cert_validation()
        .keep_alive_interval(Some(std::time::Duration::from_secs(5)))
        .build()
}

fn client_config() -> WebTransportClientConfig {
    WebTransportClientConfig {
        version: PROTOCOL_VERSION,
        lanes: AppLane::KINDS.into(),
        ..WebTransportClientConfig::new(native_config())
    }
}

//
// logic
//

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
        .init_resource::<Client>()
        .init_resource::<UiState>()
        .add_systems(Startup, setup)
        .add_systems(Update, (on_connected, on_disconnected, on_recv, ui).chain())
        .run();
}

#[derive(Debug, Default, Resource)]
struct UiState {
    url: String,
    log: Vec<String>,
    msg: String,
}

#[derive(Debug, Resource, Deref, DerefMut)]
struct ConnectSystem(SystemId<String>);

fn setup(world: &mut World) {
    #[cfg(not(target_family = "wasm"))]
    world.init_resource::<TokioRuntime>();

    let connect = world.register_system(connect);
    world.insert_resource(ConnectSystem(connect));
}

fn connect(
    In(target): In<String>,
    #[cfg(not(target_family = "wasm"))] runtime: Res<TokioRuntime>,
    mut client: ResMut<Client>,
    mut ui_state: ResMut<UiState>,
) {
    ui_state.log.push(format!("Connecting to {target}"));
    let Ok(backend) = client.connect(client_config(), target) else {
        ui_state.log.push(format!("Client is already connected"));
        return;
    };
    #[cfg(target_family = "wasm")]
    wasm_bindgen_futures::spawn_local(backend);
    #[cfg(not(target_family = "wasm"))]
    runtime.spawn(backend);
}

// update

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
    for LocalClientDisconnected { error: reason } in events.read() {
        ui_state.log.push(format!(
            "Disconnected: {:#}",
            aeronet::error::pretty_error(&reason)
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
    mut commands: Commands,
    connect_system: Res<ConnectSystem>,
    mut egui: EguiContexts,
    mut client: ResMut<Client>,
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
                    commands.run_system_with_input(**connect_system, url);
                }
            });

            ui.add_enabled_ui(!client.state().is_disconnected(), |ui| {
                if ui.button("Disconnect").clicked() {
                    ui_state.log.push(format!("Disconnected by user"));
                    let _ = client.disconnect();
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
