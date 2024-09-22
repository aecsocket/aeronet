//! Example server using WebTransport which listens for clients sending strings
//! and sends back a string reply.

// cfg_if::cfg_if! {
//     if #[cfg(target_family = "wasm")] {
//         fn main() {
//             eprintln!("this example is not for WASM");
//         }
//     } else {

use {
    aeronet_io::{
        connection::{DisconnectReason, Disconnected, Session},
        packet::PacketBuffers,
    },
    aeronet_webtransport::{
        cert,
        server::{
            ServerConfig, SessionRequest, SessionResponse, WebTransportServer,
            WebTransportServerPlugin,
        },
    },
    bevy::{log::LogPlugin, prelude::*},
    std::time::Duration,
};

fn main() -> AppExit {
    App::new()
        .add_plugins((
            MinimalPlugins,
            LogPlugin::default(),
            WebTransportServerPlugin,
        ))
        .add_systems(Startup, open_server)
        .add_systems(Update, reply)
        .observe(on_session_request)
        .observe(on_connected)
        .observe(on_disconnected)
        .run()
}

fn server_config(identity: &wtransport::Identity) -> ServerConfig {
    wtransport::ServerConfig::builder()
        .with_bind_default(25565)
        .with_identity(&identity)
        .keep_alive_interval(Some(Duration::from_secs(1)))
        .max_idle_timeout(Some(Duration::from_secs(5)))
        .unwrap()
        .build()
}

fn open_server(mut commands: Commands) {
    let identity = wtransport::Identity::self_signed(["localhost", "127.0.0.1", "::1"]).unwrap();
    let cert = &identity.certificate_chain().as_slice()[0];
    let spki_fingerprint = cert::spki_fingerprint_b64(cert).unwrap();
    let cert_hash = cert::hash_to_b64(cert.hash());
    info!("************************");
    info!("SPKI FINGERPRINT");
    info!("  {spki_fingerprint}");
    info!("CERTIFICATE HASH");
    info!("  {cert_hash}");
    info!("************************");

    let config = server_config(&identity);
    commands.spawn_empty().add(WebTransportServer::open(config));
}

fn on_session_request(
    trigger: Trigger<SessionRequest>,
    clients: Query<&Parent>,
    mut commands: Commands,
) {
    let client = trigger.entity();
    let request = trigger.event();
    let server = clients.get(client).map(Parent::get).unwrap();

    info!("{client} connecting to {server} with headers:");
    for (header_key, header_value) in &request.headers {
        info!("  {header_key}: {header_value}");
    }

    commands.trigger_targets(SessionResponse::Accepted, client);
}

fn on_connected(trigger: Trigger<OnAdd, Session>, clients: Query<&Parent>) {
    let client = trigger.entity();
    let Ok(server) = clients.get(client).map(Parent::get) else {
        return;
    };

    info!("{client} connected to {server}");
}

fn on_disconnected(trigger: Trigger<Disconnected>, clients: Query<&Parent>) {
    let client = trigger.entity();
    let Ok(server) = clients.get(client).map(Parent::get) else {
        return;
    };

    match &trigger.event().reason {
        DisconnectReason::User(reason) => {
            info!("{client} disconnected from {server} by user: {reason}");
        }
        DisconnectReason::Peer(reason) => {
            info!("{client} disconnected from {server} by peer: {reason}");
        }
        DisconnectReason::Error(err) => {
            warn!("{client} disconnected from {server} due to error: {err:#}");
        }
    }
}

fn reply(mut clients: Query<(Entity, &mut PacketBuffers), With<Parent>>) {
    for (client, mut bufs) in &mut clients {
        let mut to_send = Vec::new();

        for msg in bufs.drain_recv() {
            let msg = String::from_utf8(msg.into()).unwrap_or_else(|_| "(not UTF-8)".into());
            info!("{client} > {msg}");

            let reply = format!("You sent: {msg}");
            info!("{client} < {reply}");
            to_send.push(reply.into());
        }

        for msg in to_send {
            bufs.push_send(msg);
        }
    }
}

// }}
