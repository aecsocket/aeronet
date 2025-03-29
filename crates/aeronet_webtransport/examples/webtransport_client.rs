//! Example showing a WebTransport client which can send and receive UTF-8
//! strings.

use {
    aeronet_io::{
        Session, SessionEndpoint,
        connection::{Disconnect, Disconnected, LocalAddr, PeerAddr},
        packet::PacketRtt,
    },
    aeronet_webtransport::{
        cert,
        client::{ClientConfig, WebTransportClient, WebTransportClientPlugin},
    },
    bevy::prelude::*,
    bevy_egui::{EguiContexts, EguiPlugin, egui},
    core::mem,
    derive_more::{Deref, DerefMut},
};

fn main() -> AppExit {
    App::new()
        .add_plugins((DefaultPlugins, EguiPlugin, WebTransportClientPlugin))
        .init_resource::<Log>()
        .add_systems(Update, (global_ui, add_msgs_to_ui, session_ui))
        .add_observer(on_connecting)
        .add_observer(on_connected)
        .add_observer(on_disconnected)
        .run()
}

#[derive(Debug, Default, Deref, DerefMut, Resource)]
struct Log(Vec<String>);

#[derive(Debug, Default, Component)]
struct SessionUi {
    msg: String,
    log: Vec<String>,
}

fn on_connecting(
    trigger: Trigger<OnAdd, SessionEndpoint>,
    names: Query<&Name>,
    mut log: ResMut<Log>,
) {
    let target = trigger.target();
    let name = names
        .get(target)
        .expect("our session entity should have a name");
    log.push(format!("{name} connecting"));
}

fn on_connected(trigger: Trigger<OnAdd, Session>, names: Query<&Name>, mut log: ResMut<Log>) {
    let target = trigger.target();
    let name = names
        .get(target)
        .expect("our session entity should have a name");
    log.push(format!("{name} connected"));
}

fn on_disconnected(trigger: Trigger<Disconnected>, names: Query<&Name>, mut log: ResMut<Log>) {
    let target = trigger.target();
    let name = names
        .get(target)
        .expect("our session entity should have a name");
    log.push(match &*trigger {
        Disconnected::ByUser(reason) => {
            format!("{name} disconnected by user: {reason}")
        }
        Disconnected::ByPeer(reason) => {
            format!("{name} disconnected by peer: {reason}")
        }
        Disconnected::ByError(err) => {
            format!("{name} disconnected due to error: {err:?}")
        }
    });
}

fn global_ui(
    mut egui: EguiContexts,
    mut commands: Commands,
    mut log: ResMut<Log>,
    mut target: Local<String>,
    mut cert_hash: Local<String>,
    mut session_id: Local<usize>,
) {
    const DEFAULT_TARGET: &str = "https://127.0.0.1:25571";

    egui::Window::new("Connect").show(egui.ctx_mut(), |ui| {
        let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));

        let mut connect = false;
        ui.horizontal(|ui| {
            let connect_resp = ui.add(
                egui::TextEdit::singleline(&mut *target)
                    .hint_text(format!("{DEFAULT_TARGET} | [enter] to connect")),
            );
            connect |= connect_resp.lost_focus() && enter_pressed;
            connect |= ui.button("Connect").clicked();
        });

        let cert_hash_resp = ui.add(
            egui::TextEdit::singleline(&mut *cert_hash).hint_text("(optional) certificate hash"),
        );
        connect |= cert_hash_resp.lost_focus() && enter_pressed;

        (|| {
            if connect {
                let mut target = target.clone();
                if target.is_empty() {
                    DEFAULT_TARGET.clone_into(&mut target);
                }

                let cert_hash = cert_hash.clone();
                let config = match client_config(cert_hash) {
                    Ok(config) => config,
                    Err(err) => {
                        log.push(format!("Failed to create client config: {err:?}"));
                        return;
                    }
                };

                *session_id += 1;
                let name = format!("{}. {target}", *session_id);
                commands
                    .spawn((Name::new(name), SessionUi::default()))
                    .queue(WebTransportClient::connect(config, target));
            }
        })();

        for msg in log.iter() {
            ui.label(msg);
        }
    });
}

#[cfg(target_family = "wasm")]
fn client_config(cert_hash: String) -> Result<ClientConfig, anyhow::Error> {
    use {
        aeronet_webtransport::xwt_web::{CertificateHash, HashAlgorithm},
        anyhow::bail,
    };

    let server_certificate_hashes = if cert_hash.is_empty() {
        Vec::new()
    } else {
        match cert::hash_from_b64(&cert_hash) {
            Ok(hash) => vec![CertificateHash {
                algorithm: HashAlgorithm::Sha256,
                value: Vec::from(hash),
            }],
            Err(err) => {
                bail!("Failed to read certificate hash from string: {err:?}");
            }
        }
    };

    Ok(ClientConfig {
        server_certificate_hashes,
        ..Default::default()
    })
}

#[cfg(not(target_family = "wasm"))]
fn client_config(cert_hash: String) -> Result<ClientConfig, anyhow::Error> {
    use {aeronet_webtransport::wtransport::tls::Sha256Digest, core::time::Duration};

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

fn add_msgs_to_ui(mut sessions: Query<(&mut SessionUi, &mut Session)>) {
    for (mut ui_state, mut session) in &mut sessions {
        for packet in session.recv.drain(..) {
            let msg =
                String::from_utf8(packet.payload.into()).unwrap_or_else(|_| "(not UTF-8)".into());
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
        Option<&mut Session>,
        Option<&PacketRtt>,
        Option<&LocalAddr>,
        Option<&PeerAddr>,
    )>,
) {
    for (entity, name, mut ui_state, mut session, packet_rtt, local_addr, peer_addr) in
        &mut sessions
    {
        egui::Window::new(name.to_string()).show(egui.ctx_mut(), |ui| {
            let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));

            let mut send_msg = false;
            let msg_resp = ui
                .horizontal(|ui| {
                    if session.is_none() {
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
                if let Some(session) = &mut session {
                    let msg = mem::take(&mut ui_state.msg);
                    ui_state.log.push(format!("< {msg}"));
                    session.send.push(msg.into());
                    ui.memory_mut(|m| m.request_focus(msg_resp.id));
                }
            }

            if ui.button("Disconnect").clicked() {
                commands.trigger_targets(Disconnect::new("pressed disconnect button"), entity);
            }

            let stats = session.as_ref().map(|s| s.stats).unwrap_or_default();

            egui::Grid::new("stats").show(ui, |ui| {
                ui.label("Packet RTT");
                ui.label(
                    packet_rtt
                        .map(|PacketRtt(rtt)| format!("{rtt:?}"))
                        .unwrap_or_default(),
                );
                ui.end_row();

                ui.label("Packet MTU");
                ui.label(format!("{}", session.map(|s| s.mtu()).unwrap_or_default()));
                ui.end_row();

                ui.label("Packets recv/sent");
                ui.label(format!("{} / {}", stats.packets_recv, stats.packets_sent));
                ui.end_row();

                ui.label("Bytes recv/sent");
                ui.label(format!("{} / {}", stats.bytes_recv, stats.bytes_sent));
                ui.end_row();

                ui.label("Local address");
                ui.label(
                    local_addr
                        .map(|LocalAddr(addr)| format!("{addr:?}"))
                        .unwrap_or_default(),
                );
                ui.end_row();

                ui.label("Peer address");
                ui.label(
                    peer_addr
                        .map(|PeerAddr(addr)| format!("{addr:?}"))
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
