use {
    aeronet::{
        connection::{DisconnectReason, Disconnected, LocalAddr},
        server::Opened,
    },
    aeronet_replicon::server::{AeronetRepliconServer, AeronetRepliconServerPlugin, RepliconId},
    aeronet_websocket::{
        server::{WebSocketServer, WebSocketServerPlugin},
        tungstenite::protocol::WebSocketConfig,
    },
    aeronet_webtransport::{
        cert,
        server::{SessionRequest, SessionResponse, WebTransportServer, WebTransportServerPlugin},
        wtransport,
    },
    bevy::{log::LogPlugin, prelude::*, state::app::StatesPlugin},
    bevy_replicon::prelude::*,
    move_box::{
        ClientPlayer, MoveBoxPlugin, Player, PlayerColor, PlayerInput, PlayerPosition, TICK_RATE,
    },
    std::net::{Ipv6Addr, SocketAddr},
    web_time::Duration,
};

const WEB_TRANSPORT_PORT: u16 = 25565;

const WEB_SOCKET_PORT: u16 = 25566;

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

pub fn main() -> AppExit {
    App::new()
        .init_resource::<Args>()
        .add_plugins((
            // core
            LogPlugin::default(),
            MinimalPlugins,
            StatesPlugin,
            // transport
            WebTransportServerPlugin,
            WebSocketServerPlugin,
            // replication
            RepliconPlugins.set(ServerPlugin {
                tick_policy: TickPolicy::MaxTickRate(TICK_RATE),
                ..Default::default()
            }),
            AeronetRepliconServerPlugin,
            // game
            MoveBoxPlugin,
        ))
        .add_systems(Startup, (open_web_transport_server, open_web_socket_server))
        .observe(on_opened)
        .observe(on_session_request)
        .observe(on_connected)
        .observe(on_disconnected)
        .run()
}

//
// WebTransport
//

fn open_web_transport_server(mut commands: Commands, args: Res<Args>) {
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

    let config = web_transport_config(&identity, &args);
    let server = commands
        .spawn(AeronetRepliconServer)
        .add(WebTransportServer::open(config))
        .id();
    info!("Opening WebTransport server {server}");
}

type WebTransportServerConfig = aeronet_webtransport::server::ServerConfig;

fn web_transport_config(identity: &wtransport::Identity, args: &Args) -> WebTransportServerConfig {
    WebTransportServerConfig::builder()
        .with_bind_default(args.wt_port)
        .with_identity(&identity)
        .keep_alive_interval(Some(Duration::from_secs(1)))
        .max_idle_timeout(Some(Duration::from_secs(5)))
        .unwrap()
        .build()
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

//
// WebSocket
//

type WebSocketServerConfig = aeronet_websocket::server::ServerConfig;

fn open_web_socket_server(mut commands: Commands, args: Res<Args>) {
    let config = web_socket_config(&args);
    let server = commands
        .spawn(AeronetRepliconServer)
        .add(WebSocketServer::open(config))
        .id();
    info!("Opening WebSocket server {server}");
}

fn web_socket_config(args: &Args) -> WebSocketServerConfig {
    WebSocketServerConfig {
        addr: SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), args.ws_port),
        socket: WebSocketConfig::default(),
    }
}

//
// server logic
//

fn on_opened(trigger: Trigger<OnAdd, Opened>, servers: Query<&LocalAddr>) {
    let server = trigger.entity();
    let local_addr = servers.get(server).unwrap();
    info!("{server} opened on {}", **local_addr);
}

fn on_connected(
    trigger: Trigger<OnAdd, RepliconId>,
    clients: Query<(&Parent, &RepliconId)>,
    mut commands: Commands,
) {
    let client = trigger.entity();
    let (server, client_id) = clients.get(client).unwrap();
    let (server, client_id) = (server.get(), client_id.get());
    info!("{client} ({client_id:?}) connected to {server:?}");

    let color = Color::srgb(rand::random(), rand::random(), rand::random());
    commands.spawn((
        Player,
        ClientPlayer(client_id),
        PlayerPosition(Vec2::ZERO),
        PlayerColor(color),
        PlayerInput::default(),
        Replicated,
    ));
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

// fn print_stats(server: Res<WebTransportServer>) {
//     let now = Instant::now();
//     let mut total_mem_used = 0usize;
//     let cells = server
//         .client_keys()
//         .filter_map(|client_key| match server.client_state(client_key) {
//             ClientState::Disconnected | ClientState::Connecting(_) => None,
//             ClientState::Connected(client) => {
//                 let mem_used = client.session().memory_usage();
//                 total_mem_used += mem_used;
//                 let time = now - client.connected_at();
//                 Some(vec![
//                     format!("{:?}", slotmap::Key::data(&client_key)),
//                     format!("{:.1?}", time),
//                     format!("{:.1?}", client.rtt()),
//                     format!("{:.1?}", client.raw_rtt()),
//                     format!(
//                         "{}",
//                         fmt_bytes(
//                             (client.session().bytes_sent() as f64 / time.as_secs_f64()) as usize
//                         ),
//                     ),
//                     format!(
//                         "{}",
//                         fmt_bytes(
//                             (client.session().bytes_recv() as f64 / time.as_secs_f64()) as usize
//                         ),
//                     ),
//                     format!("{}", fmt_bytes(mem_used)),
//                 ])
//             }
//         })
//         .collect::<Vec<_>>();

//     if cells.is_empty() {
//         return;
//     }

//     let mut table = AsciiTable::default();
//     for (index, header) in ["client", "time", "rtt", "raw rtt", "tx/s", "rx/s", "mem"]
//         .iter()
//         .enumerate()
//     {
//         table.column(index).set_header(*header);
//     }

//     for line in table.format(&cells).lines() {
//         info!("{line}");
//     }

//     info!("{}B of memory used", fmt_bytes(total_mem_used));
// }

// fn fmt_bytes(n: usize) -> String {
//     format!(
//         "{:.1}",
//         SizeFormatter::<usize, BinaryPrefixes, PointSeparated>::new(n)
//     )
// }
