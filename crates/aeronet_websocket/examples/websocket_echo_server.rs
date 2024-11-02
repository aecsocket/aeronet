//! Example server using WebSocket which listens for clients sending strings
//! and sends back a string reply.

use aeronet_io::{Endpoint, Session};

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        fn main() {
            eprintln!("this example is not available on WASM");
        }
    } else {

use {
    aeronet_io::{
        connection::{DisconnectReason, Disconnected, LocalAddr},
        server::Opened,
    },
    aeronet_websocket::{
        server::{Identity, ServerConfig, WebSocketServer, WebSocketServerPlugin},
    },
    bevy::{log::LogPlugin, prelude::*},
};

fn main() -> AppExit {
    App::new()
        .add_plugins((MinimalPlugins, LogPlugin::default(), WebSocketServerPlugin))
        .add_systems(Startup, open_server)
        .add_systems(Update, reply)
        .observe(on_opened)
        .observe(on_connecting)
        .observe(on_connected)
        .observe(on_disconnected)
        .run()
}

fn server_config() -> ServerConfig {
    let identity = Identity::self_signed(["localhost", "127.0.0.1", "::1"]).expect("all given SANs should be valid DNS names");
    ServerConfig::builder()
        .with_bind_default(25565)
        .with_identity(identity)
}

fn open_server(mut commands: Commands) {
    let config = server_config();
    commands.spawn_empty().add(WebSocketServer::open(config));
}

fn on_opened(trigger: Trigger<OnAdd, Opened>, servers: Query<&LocalAddr>) {
    let server = trigger.entity();
    let local_addr = servers.get(server).expect("opened server should have a binding socket `LocalAddr`");
    info!("{server} opened on {}", **local_addr);
}

fn on_connecting(trigger: Trigger<OnAdd, Endpoint>, clients: Query<&Parent>) {
    let client = trigger.entity();
    let Ok(server) = clients.get(client).map(Parent::get) else {
        return;
    };

    info!("{client} connecting to {server}");
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
