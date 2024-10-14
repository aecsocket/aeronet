//! Example showing a WebTransport client which can send and receive UTF-8
//! strings.

use {
    aeronet_io::{
        connection::{
            Connected, Disconnect, DisconnectReason, Disconnected, LocalAddr, RemoteAddr, Session,
        },
        packet::{PacketBuffers, PacketMtu, PacketRtt, PacketStats},
    },
    aeronet_webtransport::{
        cert,
        client::{ClientConfig, WebTransportClient, WebTransportClientPlugin},
    },
    bevy::prelude::*,
    bevy_egui::{egui, EguiContexts, EguiPlugin},
    std::mem,
};

fn main() -> AppExit {
    App::new()
        .add_plugins((DefaultPlugins, EguiPlugin, WebTransportClientPlugin))
        .init_resource::<GlobalUi>()
        .add_systems(Update, (global_ui, add_msgs_to_ui, session_ui))
        .observe(on_connecting)
        .observe(on_connected)
        .observe(on_disconnected)
        .run()
}

#[derive(Debug, Default, Resource)]
struct GlobalUi {
    target: String,
    cert_hash: String,
    session_id: usize,
    log: Vec<String>,
}

#[derive(Debug, Default, Component)]
struct SessionUi {
    msg: String,
    log: Vec<String>,
}

fn on_connecting(
    trigger: Trigger<OnAdd, Session>,
    names: Query<&Name>,
    mut ui_state: ResMut<GlobalUi>,
) {
    let session = trigger.entity();
    let name = names
        .get(session)
        .expect("our session entity should have a name");
    ui_state.log.push(format!("{name} connecting"));
}

fn on_connected(
    trigger: Trigger<OnAdd, Connected>,
    names: Query<&Name>,
    mut ui_state: ResMut<GlobalUi>,
) {
    let session = trigger.entity();
    let name = names
        .get(session)
        .expect("our session entity should have a name");
    ui_state.log.push(format!("{name} connected"));
}

fn on_disconnected(
    trigger: Trigger<Disconnected>,
    names: Query<&Name>,
    mut ui_state: ResMut<GlobalUi>,
) {
    let session = trigger.entity();
    let Disconnected { reason } = trigger.event();
    let name = names
        .get(session)
        .expect("our session entity should have a name");
    ui_state.log.push(match reason {
        DisconnectReason::User(reason) => {
            format!("{name} disconnected by user: {reason}")
        }
        DisconnectReason::Peer(reason) => {
            format!("{name} disconnected by peer: {reason}")
        }
        DisconnectReason::Error(err) => {
            format!("{name} disconnected due to error: {err:#}")
        }
    });
}

fn global_ui(mut egui: EguiContexts, mut commands: Commands, mut ui_state: ResMut<GlobalUi>) {
    const DEFAULT_TARGET: &str = "https://[::1]:25565";

    egui::Window::new("Connect").show(egui.ctx_mut(), |ui| {
        let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));

        let mut connect = false;
        ui.horizontal(|ui| {
            let connect_resp = ui.add(
                egui::TextEdit::singleline(&mut ui_state.target)
                    .hint_text(format!("{DEFAULT_TARGET} | [enter] to connect")),
            );
            connect |= connect_resp.lost_focus() && enter_pressed;
            connect |= ui.button("Connect").clicked();
        });

        let cert_hash_resp = ui.add(
            egui::TextEdit::singleline(&mut ui_state.cert_hash)
                .hint_text("(optional) certificate hash"),
        );
        connect |= cert_hash_resp.lost_focus() && enter_pressed;

        (|| {
            if connect {
                let mut target = ui_state.target.clone();
                if target.is_empty() {
                    DEFAULT_TARGET.clone_into(&mut target);
                }

                let cert_hash = ui_state.cert_hash.clone();
                let config = match client_config(cert_hash) {
                    Ok(config) => config,
                    Err(err) => {
                        ui_state
                            .log
                            .push(format!("Failed to create client config: {err:#}"));
                        return;
                    }
                };

                ui_state.session_id += 1;
                let name = format!("{}. {target}", ui_state.session_id);
                commands
                    .spawn((Name::new(name), SessionUi::default()))
                    .add(WebTransportClient::connect(config, target));
            }
        })();

        for msg in &ui_state.log {
            ui.label(msg);
        }
    });
}

#[cfg(target_family = "wasm")]
fn client_config(cert_hash: String) -> Result<ClientConfig, anyhow::Error> {
    use aeronet_webtransport::xwt_web_sys::{CertificateHash, HashAlgorithm};

    let server_certificate_hashes = match cert::hash_from_b64(&cert_hash) {
        Ok(hash) => vec![CertificateHash {
            algorithm: HashAlgorithm::Sha256,
            value: Vec::from(hash),
        }],
        Err(err) => {
            warn!("Failed to read certificate hash from string: {err:?}",);
            Vec::new()
        }
    };

    Ok(ClientConfig {
        server_certificate_hashes,
        ..Default::default()
    })
}

#[cfg(not(target_family = "wasm"))]
fn client_config(cert_hash: String) -> Result<ClientConfig, anyhow::Error> {
    use {aeronet_webtransport::wtransport::tls::Sha256Digest, std::time::Duration};

    let config = ClientConfig::builder().with_bind_default();

    let config = if cert_hash.is_empty() {
        #[cfg(feature = "dangerous-configuration")]
        {
            warn!("Connecting with no certificate validation");
            config.with_no_cert_validation()
        }
        #[cfg(not(feature = "dangerous-configuration"))]
        {
            config.with_server_certificate_hashes([])
        }
    } else {
        let hash = cert::hash_from_b64(&cert_hash)?;
        config.with_server_certificate_hashes([Sha256Digest::new(hash)])
    };

    Ok(config
        .keep_alive_interval(Some(Duration::from_secs(1)))
        .max_idle_timeout(Some(Duration::from_secs(5)))
        .expect("should be a valid idle timeout")
        .build())
}

fn add_msgs_to_ui(mut sessions: Query<(&mut SessionUi, &mut PacketBuffers)>) {
    for (mut ui_state, mut bufs) in &mut sessions {
        for msg in bufs.drain_recv() {
            let msg = String::from_utf8(msg.into()).unwrap_or_else(|_| "(not UTF-8)".into());
            ui_state.log.push(format!("> {msg}"));
        }
    }
}

fn session_ui(
    mut egui: EguiContexts,
    mut commands: Commands,
    mut sessions: Query<(
        Entity,
        &Name,
        &mut SessionUi,
        &mut PacketBuffers,
        Option<&Connected>,
        Option<&PacketRtt>,
        Option<&PacketMtu>,
        Option<&PacketStats>,
        Option<&LocalAddr>,
        Option<&RemoteAddr>,
    )>,
) {
    for (
        session,
        name,
        mut ui_state,
        mut bufs,
        connected,
        packet_rtt,
        packet_mtu,
        packet_stats,
        local_addr,
        remote_addr,
    ) in &mut sessions
    {
        egui::Window::new(name.to_string()).show(egui.ctx_mut(), |ui| {
            let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));

            let mut send_msg = false;
            let msg_resp = ui
                .horizontal(|ui| {
                    if connected.is_none() {
                        ui.disable();
                    }

                    let msg_resp = ui.add(
                        egui::TextEdit::singleline(&mut ui_state.msg).hint_text("[enter] to send"),
                    );
                    send_msg |= msg_resp.lost_focus() && enter_pressed;
                    send_msg |= ui.button("Send").clicked();
                    msg_resp
                })
                .inner;

            if send_msg {
                let msg = mem::take(&mut ui_state.msg);
                ui_state.log.push(format!("< {msg}"));
                bufs.push_send(msg.into());
                ui.memory_mut(|m| m.request_focus(msg_resp.id));
            }

            if ui.button("Disconnect").clicked() {
                commands.trigger_targets(Disconnect::new("disconnected by user"), session);
            }

            egui::Grid::new("stats").show(ui, |ui| {
                ui.label("Packet RTT");
                ui.label(
                    packet_rtt
                        .map(|PacketRtt(rtt)| format!("{rtt:?}"))
                        .unwrap_or_default(),
                );
                ui.end_row();

                ui.label("Packet MTU");
                ui.label(
                    packet_mtu
                        .map(|PacketMtu(mtu)| format!("{mtu}"))
                        .unwrap_or_default(),
                );
                ui.end_row();

                ui.label("Packets recv/sent");
                ui.label(
                    packet_stats
                        .map(|stats| format!("{} / {}", stats.packets_recv, stats.packets_sent))
                        .unwrap_or_default(),
                );
                ui.end_row();

                ui.label("Bytes recv/sent");
                ui.label(
                    packet_stats
                        .map(|stats| format!("{} / {}", stats.bytes_recv, stats.bytes_sent))
                        .unwrap_or_default(),
                );
                ui.end_row();

                ui.label("Local address");
                ui.label(
                    local_addr
                        .map(|LocalAddr(addr)| format!("{addr:?}"))
                        .unwrap_or_default(),
                );
                ui.end_row();

                ui.label("Remote address");
                ui.label(
                    remote_addr
                        .map(|RemoteAddr(addr)| format!("{addr:?}"))
                        .unwrap_or_default(),
                );
                ui.end_row();
            });

            egui::ScrollArea::vertical().show(ui, |ui| {
                for msg in &ui_state.log {
                    ui.label(msg);
                }
            });
        });
    }
}
