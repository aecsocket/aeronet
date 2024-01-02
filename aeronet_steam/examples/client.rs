use std::{convert::Infallible, string::FromUtf8Error};

use aeronet::{
    ClientTransportPlugin, FromServer, LocalConnected, LocalConnecting,
    LocalDisconnected, LaneKey, Message, OnLane, TryAsBytes, TryFromBytes, TransportProtocol, LaneProtocol,
};
use aeronet_steam::SteamClientTransport;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPlugin};
use steamworks::SteamId;

// Protocol

#[derive(Debug, Clone, LaneKey)]
#[lane_kind(ReliableOrdered)]
struct AppLane;

#[derive(Debug, Clone, Message, OnLane)]
#[lane_type(AppLane)]
#[on_lane(AppLane)]
struct AppMessage(String);

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

// App

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            EguiPlugin,
            ClientTransportPlugin::<AppProtocol, SteamClientTransport<_>>::default(),
        ))
        .init_resource::<UiState>()
        .add_systems(Startup, setup)
        .add_systems(Update, (update_steam, (add_to_log, ui).chain()))
        .run();
}

#[derive(Resource)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogKind {
    Info,
    Msg,
    Connect,
    Error,
}

#[derive(Debug, Clone, Resource, Default)]
struct UiState {
    log: Vec<(LogKind, String)>,
    target: String,
}

fn add_to_log(
    mut ui_state: ResMut<UiState>,
    mut connecting: EventReader<LocalConnecting>,
    mut connected: EventReader<LocalConnected>,
    mut disconnected: EventReader<
        LocalDisconnected<AppProtocol, SteamClientTransport<AppProtocol>>,
    >,
    mut recv: EventReader<FromServer<AppProtocol>>,
) {
    for LocalConnecting in connecting.read() {
        ui_state.log.push((LogKind::Info, format!("Connecting")));
    }

    for LocalConnected in connected.read() {
        ui_state.log.push((LogKind::Connect, format!("Connected")));
    }

    for LocalDisconnected { reason } in disconnected.read() {
        ui_state
            .log
            .push((LogKind::Error, format!("Disconnected: {:#}", reason)));
    }

    for FromServer { msg, .. } in recv.read() {
        ui_state.log.push((LogKind::Msg, format!("> {}", msg.0)));
    }
}

fn ui(
    mut egui: EguiContexts,
    mut ui_state: ResMut<UiState>,
    steam: Res<SteamClient>,
    mut client: ResMut<SteamClientTransport<AppProtocol>>,
) {
    egui::Window::new("Client").show(egui.ctx_mut(), |ui| {
        ui.horizontal(|ui| {
            ui.label("Target Steam ID");
            let target_resp =
                ui.add(egui::TextEdit::singleline(&mut ui_state.target).hint_text("[enter]"));
            let mut connect = false;
            if target_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                connect = true;
            }
            connect |= ui.button("Connect").clicked();

            if connect {
                if let Err(err) = try_connect(&ui_state.target, steam.as_ref(), client.as_mut()) {
                    ui_state.log.push((LogKind::Error, err));
                }
            }
        });

        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            for (kind, line) in ui_state.log.iter() {
                ui.label(
                    egui::RichText::new(line)
                        .font(egui::FontId::monospace(14.0))
                        .color(match kind {
                            LogKind::Info => egui::Color32::WHITE,
                            LogKind::Msg => egui::Color32::GRAY,
                            LogKind::Connect => egui::Color32::GREEN,
                            LogKind::Error => egui::Color32::RED,
                        }),
                );
            }
        });
    });
}

fn try_connect(
    remote: &str,
    steam: &SteamClient,
    client: &mut SteamClientTransport<AppProtocol>,
) -> Result<(), String> {
    let remote = remote
        .parse::<u64>()
        .map_err(|err| format!("Failed to parse Steam ID: {err:#}"))?;
    let remote = SteamId::from_raw(remote);
    client
        .connect_p2p(&steam.0, remote, 0)
        .map_err(|err| format!("Failed to connect: {err:#}"))
}
