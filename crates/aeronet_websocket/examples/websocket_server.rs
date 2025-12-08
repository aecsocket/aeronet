//! Example server using WebSocket which listens for clients sending strings
//! and sends back a string reply.

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        fn main() {
            panic!("not supported on WASM");
        }
    } else {

use {
    aeronet_io::{
        Session, SessionEndpoint,
        connection::{DisconnectReason, Disconnected, LocalAddr},
        server::{Closed, Server},
    },
    aeronet_websocket::server::{Identity, ServerConfig, WebSocketServer, WebSocketServerPlugin},
    bevy::{log::LogPlugin, prelude::*},
};

fn main() -> AppExit {
    App::new()
        .add_plugins((MinimalPlugins, LogPlugin::default(), WebSocketServerPlugin))
        .add_systems(Startup, (setup_camera, open_server))
        .add_systems(Update, reply)
        .add_observer(on_opened)
        .add_observer(on_closed)
        .add_observer(on_connecting)
        .add_observer(on_connected)
        .add_observer(on_disconnected)
        .run()
}

fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

fn server_config() -> ServerConfig {
    let identity = Identity::self_signed(["localhost", "127.0.0.1", "::1"])
        .expect("all given SANs should be valid DNS names");
    ServerConfig::builder()
        .with_bind_default(25570)
        .with_identity(identity)
}

fn open_server(mut commands: Commands) {
    let config = server_config();
    commands.spawn_empty().queue(WebSocketServer::open(config));
}

fn on_closed(trigger: On<Closed>) {
    panic!("server closed: {:?}", trigger.event());
}

fn on_opened(trigger: On<Add, Server>, servers: Query<&LocalAddr>) {
    let server = trigger.event_target();
    let local_addr = servers
        .get(server)
        .expect("opened server should have a binding socket `LocalAddr`");
    info!("{server} opened on {}", **local_addr);
}

fn on_connecting(trigger: On<Add, SessionEndpoint>, clients: Query<&ChildOf>) {
    let client = trigger.event_target();
    let Ok(&ChildOf(server)) = clients.get(client) else {
        return;
    };

    info!("{client} connecting to {server}");
}

fn on_connected(trigger: On<Add, Session>, clients: Query<&ChildOf>) {
    let client = trigger.event_target();
    let Ok(&ChildOf(server)) = clients.get(client) else {
        return;
    };

    info!("{client} connected to {server}");
}

fn on_disconnected(trigger: On<Disconnected>, clients: Query<&ChildOf>) {
    let client = trigger.event_target();
    let Ok(&ChildOf(server)) = clients.get(client) else {
        return;
    };

    match &trigger.reason {
        DisconnectReason::ByUser(reason) => {
            info!("{client} disconnected from {server} by user: {reason}");
        }
        DisconnectReason::ByPeer(reason) => {
            info!("{client} disconnected from {server} by peer: {reason}");
        }
        DisconnectReason::ByError(err) => {
            warn!("{client} disconnected from {server} due to error: {err:#}");
        }
    }
}

fn reply(mut clients: Query<(Entity, &mut Session), With<ChildOf>>) {
    for (client, mut session) in &mut clients {
        // explicit deref so we can access disjoint fields
        let session = &mut *session;
        for packet in session.recv.drain(..) {
            let msg =
                String::from_utf8(packet.payload.into()).unwrap_or_else(|_| "(not UTF-8)".into());
            info!("{client} > {msg}");

            let reply = format!("You sent: {msg}");
            info!("{client} < {reply}");
            session.send.push(reply.into());
        }
    }
}

}}
