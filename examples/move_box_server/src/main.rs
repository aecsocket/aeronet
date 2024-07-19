#![doc = include_str!("../README.md")]

use aeronet::{
    client::ClientState,
    error::pretty_error,
    server::{
        RemoteClientConnecting, ServerClosed, ServerOpened, ServerTransport, ServerTransportSet,
    },
    stats::Rtt,
};
use aeronet_replicon::server::{ClientKeys, RepliconServerPlugin};
use aeronet_webtransport::{
    server::{ConnectionResponse, WebTransportServer},
    wtransport,
};
use ascii_table::AsciiTable;
use bevy::{log::LogPlugin, prelude::*, time::common_conditions::on_timer};
use bevy_replicon::{
    client::ClientPlugin,
    core::Replicated,
    prelude::RepliconChannels,
    server::{ServerEvent, ServerPlugin, TickPolicy},
    RepliconPlugins,
};
use move_box::{AsyncRuntime, MoveBoxPlugin, Player, PlayerColor, PlayerPosition};
use size::Size;
use web_time::{Duration, Instant};

const DEFAULT_PORT: u16 = 25565;

const PRINT_STATS_INTERVAL: Duration = Duration::from_millis(500);

/// `move_box` demo server
#[derive(Debug, Resource, clap::Parser)]
struct Args {
    /// Port to listen on
    #[arg(long, default_value_t = DEFAULT_PORT)]
    port: u16,
}

impl FromWorld for Args {
    fn from_world(_: &mut World) -> Self {
        <Self as clap::Parser>::parse()
    }
}

fn main() {
    App::new()
        .init_resource::<Args>()
        .add_plugins((
            MinimalPlugins,
            LogPlugin::default(),
            RepliconPlugins
                .build()
                .disable::<ClientPlugin>()
                .set(ServerPlugin {
                    tick_policy: TickPolicy::MaxTickRate(10),
                    ..Default::default()
                }),
            RepliconServerPlugin::<WebTransportServer>::default(),
            MoveBoxPlugin,
        ))
        .init_resource::<WebTransportServer>()
        .add_systems(Startup, open_server)
        .add_systems(
            PreUpdate,
            (on_opened, on_closed, on_connecting, on_server_event).after(ServerTransportSet::Recv),
        )
        .add_systems(Update, print_stats.run_if(on_timer(PRINT_STATS_INTERVAL)))
        .run();
}

fn open_server(
    mut server: ResMut<WebTransportServer>,
    tasks: Res<AsyncRuntime>,
    args: Res<Args>,
    channels: Res<RepliconChannels>,
) {
    let identity = wtransport::Identity::self_signed(["localhost", "127.0.0.1", "::1"]).unwrap();
    let cert = &identity.certificate_chain().as_slice()[0];
    let spki_fingerprint = aeronet_webtransport::cert::spki_fingerprint_base64(cert).unwrap();
    info!("*** SPKI FINGERPRINT ***");
    info!("{spki_fingerprint}");
    info!("************************");

    let net_config = wtransport::ServerConfig::builder()
        .with_bind_default(args.port)
        .with_identity(&identity)
        .keep_alive_interval(Some(Duration::from_secs(1)))
        .max_idle_timeout(Some(Duration::from_secs(5)))
        .unwrap()
        .build();

    let session_config = move_box::base_session_config()
        .with_send_lanes(channels.server_channels())
        .with_recv_lanes(channels.client_channels());

    let backend = server.open(net_config, session_config).unwrap();
    tasks.spawn(backend);
}

fn on_opened(mut events: EventReader<ServerOpened<WebTransportServer>>) {
    for ServerOpened { .. } in events.read() {
        info!("Server opened");
    }
}

fn on_closed(mut events: EventReader<ServerClosed<WebTransportServer>>) {
    for ServerClosed { error } in events.read() {
        info!("Server closed: {:#}", pretty_error(&error));
    }
}

fn on_connecting(
    mut events: EventReader<RemoteClientConnecting<WebTransportServer>>,
    mut server: ResMut<WebTransportServer>,
) {
    for RemoteClientConnecting { client_key } in events.read() {
        info!("{client_key} connecting");
        let _ = server.respond_to_request(*client_key, ConnectionResponse::Accept);
    }
}

fn on_server_event(
    mut commands: Commands,
    mut events: EventReader<ServerEvent>,
    client_keys: Res<ClientKeys<WebTransportServer>>,
    players: Query<(Entity, &Player)>,
) {
    for event in events.read() {
        match event {
            ServerEvent::ClientConnected { client_id } => {
                let client_key = client_keys.get_by_right(client_id).unwrap();
                info!("{client_id:?} controlled by {client_key} connected");
                let color = Color::rgb(rand::random(), rand::random(), rand::random());
                commands.spawn((
                    Player(*client_id),
                    PlayerPosition(Vec2::ZERO),
                    PlayerColor(color),
                    Replicated,
                ));
            }
            ServerEvent::ClientDisconnected { client_id, reason } => {
                info!("{client_id:?} disconnected: {reason}");
                for (entity, Player(player)) in &players {
                    if *player == *client_id {
                        commands.entity(entity).despawn();
                    }
                }
            }
        }
    }
}

fn print_stats(server: Res<WebTransportServer>) {
    let now = Instant::now();
    let mut total_mem_used = 0usize;
    let cells = server
        .client_keys()
        .filter_map(|client_key| match server.client_state(client_key) {
            ClientState::Disconnected | ClientState::Connecting(_) => None,
            ClientState::Connected(client) => {
                client.session.recv_lanes_mem();
                let mem_used = client.session.memory_used();
                total_mem_used += mem_used;
                let time = now - client.connected_at;
                Some(vec![
                    format!("{}", client_key),
                    format!("{:.1?}", time),
                    format!("{:.1?}", client.rtt()),
                    format!("{:.1?}", client.raw_rtt),
                    format!(
                        "{}",
                        Size::from_bytes(
                            (client.session.bytes_sent() as f64 / time.as_secs_f64()) as usize
                        ),
                    ),
                    format!(
                        "{}",
                        Size::from_bytes(
                            (client.session.bytes_recv() as f64 / time.as_secs_f64()) as usize
                        ),
                    ),
                    format!("{}", Size::from_bytes(mem_used)),
                ])
            }
        })
        .collect::<Vec<_>>();

    if cells.is_empty() {
        return;
    }

    let mut table = AsciiTable::default();
    for (index, header) in ["client", "time", "rtt", "raw rtt", "tx/s", "rx/s", "mem"]
        .iter()
        .enumerate()
    {
        table.column(index).set_header(*header);
    }

    for line in table.format(&cells).lines() {
        info!("{line}");
    }

    info!("{} of memory used", Size::from_bytes(total_mem_used));
}
