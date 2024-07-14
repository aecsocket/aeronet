use aeronet::{
    client::{client_connected, ClientEvent, ClientTransport},
    error::pretty_error,
    lane::{LaneKey, LaneKind},
};
use aeronet_proto::session::{LaneConfig, SessionConfig};
use aeronet_webtransport::client::{ClientConfig, WebTransportClient};
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPlugin};

#[derive(Debug, Clone, Copy, LaneKey)]
enum Lane {
    #[lane_kind(ReliableOrdered)]
    Default,
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
        app.add_plugins(bevy_tokio_tasks::TokioTasksPlugin::default());
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
        .keep_alive_interval(Some(Duration::from_secs(1)))
        .max_idle_timeout(Some(Duration::from_secs(5)))
        .unwrap()
        .build()
}

fn session_config() -> SessionConfig {
    let lanes = vec![LaneConfig::new(LaneKind::ReliableOrdered)];
    SessionConfig {
        send_lanes: lanes.clone(),
        recv_lanes: lanes,
        default_packet_cap: 0,
        max_packet_len: 1024,
        send_cap: usize::MAX,
        recv_frags_cap: usize::MAX,
    }
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
    #[cfg(not(target_family = "wasm"))] rt: Res<bevy_tokio_tasks::TokioTasksRuntime>,
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
                        rt.runtime().spawn(backend);
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
            ui_state.log.push(match client.disconnect() {
                Ok(()) => format!("Disconnected by user"),
                Err(err) => format!("Failed to disconnect: {:#}", pretty_error(&err)),
            });
        }

        if do_send {
            ui.memory_mut(|m| m.request_focus(msg_resp.id));
            let msg = std::mem::take(&mut ui_state.msg);
            ui_state.log.push(format!("< {msg}"));
            if let Err(err) = client.send(msg, Lane::Default) {
                ui_state
                    .log
                    .push(format!("Failed to send message: {:#}", pretty_error(&err)));
            }
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            for line in &ui_state.log {
                ui.label(egui::RichText::new(line).font(egui::FontId::monospace(14.0)));
            }
        });
    });
}
