//! See `src/move_box.rs`.

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        fn main() {
            panic!("not supported on WASM");
        }
    } else {

use {
    aeronet::io::{
        Session,
        connection::{Disconnected, LocalAddr},
        server::Server,
    },
    aeronet_replicon::server::{AeronetRepliconServer, AeronetRepliconServerPlugin},
    aeronet_websocket::server::{WebSocketServer, WebSocketServerPlugin},
    aeronet_webtransport::{
        cert,
        server::{SessionRequest, SessionResponse, WebTransportServer, WebTransportServerPlugin},
        wtransport,
    },
    bevy::{app::ScheduleRunnerPlugin, log::LogPlugin, prelude::*, state::app::StatesPlugin},
    bevy_replicon::prelude::*,
    core::time::Duration,
    examples::move_box::{
        MoveBoxPlugin, Player, PlayerColor, PlayerInput, PlayerPosition, TICK_RATE,
        WEB_SOCKET_PORT, WEB_TRANSPORT_PORT,
    },
    std::time::SystemTime,
};

/// `move_box` demo server
#[derive(Debug, Resource, clap::Parser)]
struct Args {
    /// Port to listen for WebTransport connections on
    #[arg(long, default_value_t = WEB_TRANSPORT_PORT)]
    wt_port: u16,
    /// Port to listen for WebSocket connections on
    #[arg(long, default_value_t = WEB_SOCKET_PORT)]
    ws_port: u16,
}

impl FromWorld for Args {
    fn from_world(_: &mut World) -> Self {
        <Self as clap::Parser>::parse()
    }
}

fn main() -> AppExit {
    App::new()
        .init_resource::<Args>()
        .add_plugins((
            // core
            LogPlugin::default(),
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
                1.0 / f64::from(TICK_RATE),
            ))),
            StatesPlugin,
            // transport
            WebTransportServerPlugin,
            WebSocketServerPlugin,
            // replication
            RepliconPlugins.set(ServerPlugin {
                // 1 frame lasts `1.0 / TICK_RATE` anyway
                tick_policy: TickPolicy::EveryFrame,
                ..Default::default()
            }),
            AeronetRepliconServerPlugin,
            // game
            MoveBoxPlugin,
        ))
        .add_systems(Startup, (open_web_transport_server, open_web_socket_server))
        .add_observer(on_opened)
        .add_observer(on_session_request)
        .add_observer(on_connected)
        .add_observer(on_disconnected)
        .run()
}

//
// WebTransport
//

fn open_web_transport_server(mut commands: Commands, args: Res<Args>) {
    let identity = wtransport::Identity::self_signed(["localhost", "127.0.0.1", "::1"])
        .expect("all given SANs should be valid DNS names");
    let cert = &identity.certificate_chain().as_slice()[0];
    let spki_fingerprint = cert::spki_fingerprint_b64(cert).expect("should be a valid certificate");
    let cert_hash = cert::hash_to_b64(cert.hash());
    info!("************************");
    info!("SPKI FINGERPRINT");
    info!("  {spki_fingerprint}");
    info!("CERTIFICATE HASH");
    info!("  {cert_hash}");
    info!("************************");

    let config = web_transport_config(identity, &args);
    let server = commands
        .spawn((Name::new("WebTransport Server"), AeronetRepliconServer))
        .queue(WebTransportServer::open(config))
        .id();
    info!("Opening WebTransport server {server}");
}

type WebTransportServerConfig = aeronet_webtransport::server::ServerConfig;

fn web_transport_config(identity: wtransport::Identity, args: &Args) -> WebTransportServerConfig {
    WebTransportServerConfig::builder()
        .with_bind_default(args.wt_port)
        .with_identity(identity)
        .keep_alive_interval(Some(Duration::from_secs(1)))
        .max_idle_timeout(Some(Duration::from_secs(5)))
        .expect("should be a valid idle timeout")
        .build()
}

fn on_session_request(mut request: Trigger<SessionRequest>, clients: Query<&ChildOf>) {
    let client = request.target();
    let Ok(&ChildOf { parent: server }) = clients.get(client) else {
        return;
    };

    info!("{client} connecting to {server} with headers:");
    for (header_key, header_value) in &request.headers {
        info!("  {header_key}: {header_value}");
    }

    request.respond(SessionResponse::Accepted);
}

//
// WebSocket
//

type WebSocketServerConfig = aeronet_websocket::server::ServerConfig;

fn open_web_socket_server(mut commands: Commands, args: Res<Args>) {
    let config = web_socket_config(&args);
    let server = commands
        .spawn((Name::new("WebSocket Server"), AeronetRepliconServer))
        .queue(WebSocketServer::open(config))
        .id();
    info!("Opening WebSocket server {server}");
}

fn web_socket_config(args: &Args) -> WebSocketServerConfig {
    WebSocketServerConfig::builder()
        .with_bind_default(args.ws_port)
        .with_no_encryption()
}

//
// server logic
//

fn on_opened(trigger: Trigger<OnAdd, Server>, servers: Query<&LocalAddr>) {
    let server = trigger.target();
    let local_addr = servers
        .get(server)
        .expect("opened server should have a binding socket `LocalAddr`");
    info!("{server} opened on {}", **local_addr);
}

fn on_connected(
    trigger: Trigger<OnAdd, Session>,
    clients: Query<&ChildOf>,
    mut commands: Commands,
) {
    let client = trigger.target();
    let Ok(&ChildOf { parent: server }) = clients.get(client) else {
        return;
    };
    info!("{client} connected to {server}");

    // generate a random-looking color
    let time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("current system time should be after unix epoch")
        .as_millis();
    #[expect(
        clippy::cast_possible_truncation,
        reason = "truncation is what we want"
    )]
    let color = Color::srgb_u8((time * 3) as u8, (time * 5) as u8, (time * 7) as u8);

    commands.entity(client).insert((
        Player,
        PlayerPosition(Vec2::ZERO),
        PlayerColor(color),
        PlayerInput::default(),
        Replicated,
    ));
}

fn on_disconnected(trigger: Trigger<Disconnected>, clients: Query<&ChildOf>) {
    let client = trigger.target();
    let Ok(&ChildOf { parent: server }) = clients.get(client) else {
        return;
    };

    match &*trigger {
        Disconnected::ByUser(reason) => {
            info!("{client} disconnected from {server} by user: {reason}");
        }
        Disconnected::ByPeer(reason) => {
            info!("{client} disconnected from {server} by peer: {reason}");
        }
        Disconnected::ByError(err) => {
            warn!("{client} disconnected from {server} due to error: {err:?}");
        }
    }
}

}}
