use std::{convert::Infallible, mem, string::FromUtf8Error};

use aeronet::{
    ClientTransport, ClientTransportPlugin, FromServer, LaneKey, LaneProtocol, LocalConnected,
    LocalDisconnected, Message, OnLane, TransportProtocol, TryAsBytes, TryFromBytes,
};
use aeronet_steam::SteamClientTransport;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPlugin};

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

#[derive(Debug, Clone, Resource, Default)]
struct UiState {
    log: Vec<String>,
    addr: String,
    msg: String,
}

fn add_to_log(
    mut ui_state: ResMut<UiState>,
    mut connected: EventReader<LocalConnected<AppProtocol, SteamClientTransport<AppProtocol>>>,
    mut disconnected: EventReader<
        LocalDisconnected<AppProtocol, SteamClientTransport<AppProtocol>>,
    >,
    mut recv: EventReader<FromServer<AppProtocol>>,
) {
    for LocalConnected { .. } in connected.read() {
        ui_state.log.push(format!("Connected"));
    }

    for LocalDisconnected { reason } in disconnected.read() {
        ui_state.log.push(format!("Disconnected: {:#}", reason));
    }

    for FromServer { msg, .. } in recv.read() {
        ui_state.log.push(format!("> {}", msg.0));
    }
}

fn ui(
    mut egui: EguiContexts,
    mut ui_state: ResMut<UiState>,
    steam: Res<SteamClient>,
    mut client: ResMut<SteamClientTransport<AppProtocol>>,
) {
    egui::Window::new("Client").show(egui.ctx_mut(), |ui| {
        connection(client.as_mut(), steam.as_ref(), ui_state.as_mut(), ui);

        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            for line in ui_state.log.iter() {
                ui.label(egui::RichText::new(line).font(egui::FontId::monospace(14.0)));
            }
        });

        ui.separator();

        sending(client.as_mut(), ui_state.as_mut(), ui);
    });
}

fn connection(
    client: &mut SteamClientTransport<AppProtocol>,
    steam: &SteamClient,
    ui_state: &mut UiState,
    ui: &mut egui::Ui,
) {
    ui.horizontal(|ui| {
        ui.label("IP Address");

        let addr_resp = ui.add_enabled(
            client.state().is_disconnected(),
            egui::TextEdit::singleline(&mut ui_state.addr).hint_text("127.0.0.1:27015 [enter]"),
        );

        if client.state().is_disconnected() {
            let mut connect = false;
            connect |= addr_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            connect |= ui.button("Connect").clicked();

            if connect {
                let addr = match ui_state.addr.parse() {
                    Ok(addr) => addr,
                    Err(err) => {
                        ui_state.log.push(format!(
                            "Failed to parse socket address: {:#}",
                            aeronet::util::as_pretty(&err),
                        ));
                        return;
                    }
                };

                match client.connect_ip(steam.0.clone(), addr) {
                    Ok(()) => ui_state.log.push(format!("Connecting to {addr:?}")),
                    Err(err) => ui_state.log.push(format!(
                        "Failed to connect to {addr:?}: {:#}",
                        aeronet::util::as_pretty(&err)
                    )),
                }
            }
        } else {
            if ui.button("Disconnect").clicked() {
                ui_state.log.push(format!("Disconnected by user"));
                let _ = client.disconnect();
            }
        }
    });
}

fn sending(
    client: &mut SteamClientTransport<AppProtocol>,
    ui_state: &mut UiState,
    ui: &mut egui::Ui,
) {
    ui.horizontal(|ui| {
        ui.add_enabled_ui(client.state().is_connected(), |ui| {
            ui.label("Message");
            let msg_resp =
                ui.add(egui::TextEdit::singleline(&mut ui_state.msg).hint_text("[enter]"));

            let mut send = false;
            send |= msg_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            send |= ui.button("Send").clicked();

            if send {
                ui.memory_mut(|m| m.request_focus(msg_resp.id));
                let msg = mem::take(&mut ui_state.msg);
                if msg.is_empty() {
                    return;
                }

                let log = match client.send(AppMessage(msg.clone())) {
                    Ok(()) => format!("< {msg}"),
                    Err(err) => format!(
                        "Failed to send message: {:#}",
                        aeronet::util::as_pretty(&err)
                    ),
                };
                ui_state.log.push(log);
            }
        });
    });
}
