//! Example server using WebSocket which listens for clients sending strings
//! and sends back a string reply.

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};

use aeronet_io::{
    connection::{LocalAddr, Session},
    server::Opened,
};
use aeronet_websocket::{
    server::{ServerConfig, WebSocketServer, WebSocketServerPlugin},
    tungstenite::protocol::WebSocketConfig,
};

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        fn main() {
            eprintln!("this example is not for WASM");
        }
    } else {

use {
    aeronet_io::{
        connection::{Connected, DisconnectReason, Disconnected},
        packet::PacketBuffers,
    },
    bevy::{log::LogPlugin, prelude::*},
};

fn main() -> AppExit {
    App::new()
        .add_plugins((
            MinimalPlugins,
            LogPlugin::default(),
            WebSocketServerPlugin,
        ))
        .add_systems(Startup, open_server)
        .add_systems(Update, reply)
        .observe(on_opened)
        .observe(on_connecting)
        .observe(on_connected)
        .observe(on_disconnected)
        .run()
}

fn server_config() -> ServerConfig {
    ServerConfig {
        addr: SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), 25565),
        socket: WebSocketConfig::default(),
    }
}

fn open_server(mut commands: Commands) {
    let config = server_config();
    commands.spawn_empty().add(WebSocketServer::open(config));
}

fn on_opened(trigger: Trigger<OnAdd, Opened>, servers: Query<&LocalAddr>) {
    let server = trigger.entity();
    let local_addr = servers.get(server).unwrap();
    info!("{server} opened on {}", **local_addr);
}

fn on_connecting(trigger: Trigger<OnAdd, Session>, clients: Query<&Parent>) {
    let client = trigger.entity();
    let server = clients.get(client).map(Parent::get).unwrap();

    info!("{client} connecting to {server}");
}

fn on_connected(trigger: Trigger<OnAdd, Connected>, clients: Query<&Parent>) {
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

}}
