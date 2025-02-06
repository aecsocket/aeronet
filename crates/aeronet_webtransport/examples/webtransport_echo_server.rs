//! Example server using WebTransport which listens for clients sending strings
//! and sends back a string reply.

use aeronet_io::{Session, server::Server};

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        fn main() {
            eprintln!("this example is not available on WASM");
        }
    } else {

use {
    aeronet_io::{
        connection::{DisconnectReason, Disconnected, LocalAddr},
    },
    aeronet_webtransport::{
        cert,
        server::{
            ServerConfig, SessionRequest, SessionResponse, WebTransportServer,
            WebTransportServerPlugin,
        },
    },
    bevy::{log::LogPlugin, prelude::*},
    core::time::Duration,
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
        .add_observer(on_opened)
        .add_observer(on_session_request)
        .add_observer(on_connected)
        .add_observer(on_disconnected)
        .run()
}

fn open_server(mut commands: Commands) {
    let identity = wtransport::Identity::self_signed(["localhost", "127.0.0.1", "::1"]).expect("all given SANs should be valid DNS names");
    let cert = &identity.certificate_chain().as_slice()[0];
    let spki_fingerprint = cert::spki_fingerprint_b64(cert).expect("should be a valid certificate");
    let cert_hash = cert::hash_to_b64(cert.hash());
    info!("************************");
    info!("SPKI FINGERPRINT");
    info!("  {spki_fingerprint}");
    info!("CERTIFICATE HASH");
    info!("  {cert_hash}");
    info!("************************");

    let config = server_config(&identity);
    commands.spawn_empty().queue(WebTransportServer::open(config));
}

fn server_config(identity: &wtransport::Identity) -> ServerConfig {
    wtransport::ServerConfig::builder()
        .with_bind_default(25565)
        .with_identity(identity)
        .keep_alive_interval(Some(Duration::from_secs(1)))
        .max_idle_timeout(Some(Duration::from_secs(5)))
        .expect("should be a valid idle timeout")
        .build()
}

fn on_opened(trigger: Trigger<OnAdd, Server>, servers: Query<&LocalAddr>) {
    let server = trigger.entity();
    let local_addr = servers.get(server)
        .expect("spawned session entity should have a name");
    info!("{server} opened on {}", **local_addr);
}

fn on_session_request(
    mut request: Trigger<SessionRequest>,
    clients: Query<&Parent>,
) {
    let client = request.entity();
    let Ok(server) = clients.get(client).map(Parent::get) else {
        return;
    };

    info!("{client} connecting to {server} with headers:");
    for (header_key, header_value) in &request.headers {
        info!("  {header_key}: {header_value}");
    }

    request.respond(SessionResponse::Accepted);
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

    match &trigger.reason {
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

fn reply(mut clients: Query<(Entity, &mut Session), With<Parent>>) {
    for (client, mut session) in &mut clients {
        // explicit deref so we can access disjoint fields
        let session = &mut *session;
        for packet in session.recv.drain(..) {
            let msg = String::from_utf8(packet.payload.into()).unwrap_or_else(|_| "(not UTF-8)".into());
            info!("{client} > {msg}");

            let reply = format!("You sent: {msg}");
            info!("{client} < {reply}");
            session.send.push(reply.into());
        }
    }
}

}}
