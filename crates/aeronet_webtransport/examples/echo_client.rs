//! Example client using WebTransport which allows sending a string to a server
//! and reading a string back.

use aeronet::{
    client::{client_connected, ClientEvent, ClientState, ClientTransport},
    error::pretty_error,
    lane::{LaneIndex, LaneKind},
    stats::{MessageStats, Rtt},
};
use aeronet_proto::session::SessionConfig;
use aeronet_webtransport::client::{ClientConfig, WebTransportClient};
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPlugin};

#[cfg(not(target_family = "wasm"))]
#[derive(Debug, Resource)]
struct TokioRuntime(tokio::runtime::Runtime);

#[cfg(not(target_family = "wasm"))]
impl FromWorld for TokioRuntime {
    fn from_world(_: &mut World) -> Self {
        Self(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap(),
        )
    }
}

#[derive(Debug, Clone, Copy)]
struct AppLane;

impl From<AppLane> for LaneKind {
    fn from(_: AppLane) -> Self {
        LaneKind::ReliableOrdered
    }
}

impl From<AppLane> for LaneIndex {
    fn from(_: AppLane) -> Self {
        Self::from_raw(0)
    }
}

#[derive(Debug, Default, Resource)]
struct UiState {
    log: Vec<String>,
    target: String,
    msg: String,
}

fn main() {
    let mut app = App::new();
    app.add_plugins((DefaultPlugins, EguiPlugin))
        .init_resource::<UiState>()
        .init_resource::<WebTransportClient>()
        .add_systems(PreUpdate, poll_client)
        .add_systems(Update, ui)
        .add_systems(
            PostUpdate,
            flush_client.run_if(client_connected::<WebTransportClient>),
        );

    #[cfg(not(target_family = "wasm"))]
    {
        app.init_resource::<TokioRuntime>();
    }

    app.run();
}

#[cfg(target_family = "wasm")]
fn client_config() -> ClientConfig {
    ClientConfig::default()
}

#[cfg(not(target_family = "wasm"))]
fn client_config() -> ClientConfig {
    use web_time::Duration;

    ClientConfig::builder()
        .with_bind_default()
        .with_no_cert_validation()
        .max_idle_timeout(Some(Duration::from_secs(5)))
        .unwrap()
        .build()
}

fn session_config() -> SessionConfig {
    // configure both the sending and receiving lanes
    // we will buffer up to 4MB of the server's fragments at once
    // TODO is 4MB a reasonable number?
    SessionConfig::new(1024 * 1024 * 4).with_lanes([AppLane])
}

fn poll_client(
    time: Res<Time>,
    mut client: ResMut<WebTransportClient>,
    mut ui_state: ResMut<UiState>,
) {
    for event in client.poll(time.delta()) {
        match event {
            ClientEvent::Connected => {
                ui_state.log.push(format!("Connected"));
            }
            ClientEvent::Disconnected { error } => {
                ui_state
                    .log
                    .push(format!("Disconnected: {:#}", pretty_error(&error)));
            }
            ClientEvent::Recv { msg, .. } => {
                let msg =
                    String::from_utf8(msg.into()).unwrap_or_else(|_| format!("<invalid UTF-8>"));
                ui_state.log.push(format!("> {msg}"));
            }
            ClientEvent::Ack { .. } | ClientEvent::Nack { .. } => {}
        }
    }
}

fn flush_client(mut client: ResMut<WebTransportClient>, mut ui_state: ResMut<UiState>) {
    if let Err(err) = client.flush() {
        ui_state.log.push(format!(
            "Failed to flush messages: {:#}",
            pretty_error(&err)
        ));
    }
}

fn ui(
    mut egui: EguiContexts,
    mut ui_state: ResMut<UiState>,
    mut client: ResMut<WebTransportClient>,
    #[cfg(not(target_family = "wasm"))] rt: Res<TokioRuntime>,
) {
    egui::Window::new("Client").show(egui.ctx_mut(), |ui| {
        let pressed_enter = ui.input(|i| i.key_pressed(egui::Key::Enter));

        let mut do_connect = false;
        let mut do_disconnect = false;
        ui.horizontal(|ui| {
            let target_resp = ui.add_enabled(
                client.state().is_disconnected(),
                egui::TextEdit::singleline(&mut ui_state.target).hint_text("https://[::1]:25565"),
            );

            if client.state().is_disconnected() {
                do_connect |= target_resp.lost_focus() && pressed_enter;
                do_connect |= ui.button("Connect").clicked();
            } else {
                do_disconnect |= ui.button("Disconnect").clicked();
            }
        });

        let mut do_send = false;
        let msg_resp = ui
            .add_enabled_ui(client.state().is_connected(), |ui| {
                ui.horizontal(|ui| {
                    let msg_resp = ui.add(
                        egui::TextEdit::singleline(&mut ui_state.msg).hint_text("[enter] to send"),
                    );
                    do_send |= msg_resp.lost_focus() && pressed_enter;
                    do_send |= ui.button("Send").clicked();
                    msg_resp
                })
                .inner
            })
            .inner;

        if do_connect {
            ui.memory_mut(|m| m.request_focus(msg_resp.id));
            let target = ui_state.target.clone();
            ui_state.log.push(format!("Connecting to {target}"));
            match client.connect(client_config(), session_config(), target) {
                Ok(backend) => {
                    #[cfg(target_family = "wasm")]
                    {
                        wasm_bindgen_futures::spawn_local(backend);
                    }
                    #[cfg(not(target_family = "wasm"))]
                    {
                        rt.0.spawn(backend);
                    }
                }
                Err(err) => {
                    ui_state.log.push(format!(
                        "Failed to start connecting: {:#}",
                        pretty_error(&err)
                    ));
                }
            }
        }

        if do_disconnect {
            ui_state.log.push(format!("Disconnected by user"));
            let _ = client.disconnect();
        }

        if do_send {
            ui.memory_mut(|m| m.request_focus(msg_resp.id));
            let msg = std::mem::take(&mut ui_state.msg);
            if !msg.is_empty() {
                ui_state.log.push(format!("< {msg}"));
                let _ = client.send(msg, AppLane);
            }
        }

        if let ClientState::Connected(client) = client.state() {
            egui::Grid::new("meta").num_columns(2).show(ui, |ui| {
                ui.label("Local/remote addr");
                ui.label(format!("{} / {}", client.local_addr, client.remote_addr));
                ui.end_row();

                ui.label("RTT");
                ui.label(format!("{:?} ({:?} raw)", client.rtt(), client.raw_rtt));
                ui.end_row();

                ui.label("Bytes sent/recv");
                ui.label(format!("{} / {}", client.bytes_sent(), client.bytes_recv()));
                ui.end_row();

                ui.label("Bytes left / cap");
                ui.label(format!(
                    "{} / {}",
                    client.session.bytes_left().get(),
                    client.session.bytes_left().cap()
                ));
                ui.end_row();

                ui.label("MTU min / current");
                ui.label(format!(
                    "{} / {}",
                    client.session.min_mtu(),
                    client.session.mtu()
                ));
                ui.end_row();
            });
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            for line in &ui_state.log {
                ui.label(egui::RichText::new(line).font(egui::FontId::monospace(14.0)));
            }
        });
    });
}
