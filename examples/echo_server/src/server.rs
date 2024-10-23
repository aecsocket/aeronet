use {
    aeronet::{
        io::{
            bytes::Bytes,
            connection::{Connected, DisconnectReason, Disconnected, LocalAddr, Session},
            server::Opened,
        },
        transport::{lane::LaneKind, AeronetTransportPlugin, Transport},
    },
    aeronet_websocket::server::{ServerConfig, WebSocketServer, WebSocketServerPlugin},
    bevy::{log::LogPlugin, prelude::*},
};

// Let's set up the app.

pub fn main() -> AppExit {
    App::new()
        .add_plugins((
            // Core Bevy plugins.
            LogPlugin::default(),
            MinimalPlugins,
            // We're using WebSockets, so we add this plugin.
            // This will automatically add `AeronetIoPlugin` as well, which sets
            // up the IO layer. However, it does *not* set up the transport
            // layer (since technically, you may want to swap it out and use
            // your own).
            WebSocketServerPlugin,
            // Here we actually set up the transport layer.
            AeronetTransportPlugin,
        ))
        // Open the server on startup.
        .add_systems(Startup, setup)
        // Every frame, we receive messages and print them out.
        .add_systems(Update, echo_messages)
        // Set up some observers to run when the server or client state changes.
        .observe(on_opened)
        .observe(on_connected)
        .observe(on_disconnected)
        .run()
}

// Use a fixed listen port for this example, mapping to the URL that the
// `echo_client` connects to.
const LISTEN_PORT: u16 = 25566;

// Define what `aeronet_transport` lanes will be used on client connections.
// When using the transport layer, you must define in advance what lanes will be
// available.
// The receiving and sending lanes may be different, but in this example we will
// use the same lane configuration for both.
const LANES: [LaneKind; 1] = [LaneKind::ReliableOrdered];

fn setup(mut commands: Commands) {
    // Let's set up our WebSocket server.

    // First we need to handle encryption. For this, we generate some
    // self-signed certificates to identify ourselves.
    // Clients won't be able to connect to our server with self-signed certs
    // unless they disable cert validation (the demo client does this).
    let identity =
        aeronet_websocket::server::Identity::self_signed(["localhost", "127.0.0.1", "::1"])
            .expect("all given SANs should be valid DNS names");

    let config = ServerConfig::builder()
        .with_bind_default(LISTEN_PORT)
        .with_identity(identity);
    // Spawn an entity to represent this server.
    let mut server = commands.spawn_empty();
    // Make an `EntityCommand` via `open`, which will set up and open this
    // server.
    server.add(WebSocketServer::open(config));
}

// Observe state change events using `Trigger`s
fn on_opened(trigger: Trigger<OnAdd, Opened>, servers: Query<&LocalAddr>) {
    let server = trigger.entity();
    let local_addr = servers
        .get(server)
        .expect("opened server should have a binding socket `LocalAddr`");
    info!("{server} opened on {}", **local_addr);
}

fn on_connected(
    trigger: Trigger<OnAdd, Connected>,
    clients: Query<&Parent>,
    mut commands: Commands,
) {
    let client = trigger.entity();
    // A `Connected` `Session` which has a `Parent` is a client of a server.
    let Ok(server) = clients.get(client).map(Parent::get) else {
        return;
    };
    info!("{client} connected to {server}");

    // Add `Transport` and configure it with our lanes so that we can send
    // and receive messages on this client.
    commands.entity(client).insert(Transport::new(LANES, LANES));
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

// Receive messages and echo them back to the sender.
fn echo_messages(
    // Query..
    mut clients: Query<
        (
            Entity,         // ..the entity ID
            &mut Transport, // ..and the transport layer access
        ),
        (
            With<Session>,   // ..for all sessions (this isn't strictly necessary)
            With<Connected>, // which are connected (this isn't strictly necessary)
            With<Parent>,    /* ..which are connected to one of our servers (excludes local
                              * dedicated clients) */
        ),
    >,
) {
    for (client, mut transport) in &mut clients {
        // Explicitly deref the `Mut<Transport>` to get a `&mut Transport`
        // from which we can grab disjoint refs to `recv` and `send`.
        let transport = &mut *transport;

        for (lane_index, msg) in transport.recv.drain() {
            // `msg` is a `Vec<u8>` - we have full ownership of the bytes received.
            // We'll turn it into a UTF-8 string, and resend it along the same
            // lane that we received it on.
            let msg = String::from_utf8(msg).unwrap_or_else(|_| "(not UTF-8)".into());
            info!("{client} > {msg}");

            let reply = format!("You sent: {msg}");
            info!("{client} < {reply}");
            // Convert our `String` into a `Bytes` to send it out.
            // We ignore the resulting `MessageKey`, since we don't need it.
            _ = transport.send.push(lane_index, Bytes::from(reply));
        }
    }
}
