#![doc = include_str!("../README.md")]

use aeronet::{
    error::pretty_error,
    server::{RemoteClientConnecting, ServerClosed, ServerOpened, ServerTransportSet},
};
use aeronet_replicon::server::RepliconServerPlugin;
use aeronet_webtransport::{
    server::{ConnectionResponse, WebTransportServer},
    wtransport,
};
use bevy::{log::LogPlugin, prelude::*};
use bevy_replicon::{
    core::Replicated, prelude::RepliconChannels, server::ServerEvent, RepliconPlugins,
};
use move_box::{MoveBoxPlugin, Player, PlayerColor, PlayerPosition};
use web_time::Duration;

/// `move_box` demo server
#[derive(Debug, Resource, clap::Parser)]
struct Args {
    /// Port to listen on
    #[arg(long, default_value_t = DEFAULT_PORT)]
    port: u16,
}

const DEFAULT_PORT: u16 = 25565;

impl FromWorld for Args {
    fn from_world(_: &mut World) -> Self {
        <Self as clap::Parser>::parse()
    }
}

#[derive(Debug, Deref, DerefMut, Resource)]
struct TokioRuntime(tokio::runtime::Runtime);

impl FromWorld for TokioRuntime {
    fn from_world(_: &mut World) -> Self {
        Self(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap(),
        )
    }
}

fn main() {
    App::new()
        .init_resource::<Args>()
        .add_plugins((
            MinimalPlugins,
            LogPlugin::default(),
            RepliconPlugins,
            RepliconServerPlugin::<WebTransportServer>::default(),
            MoveBoxPlugin,
        ))
        .init_resource::<TokioRuntime>()
        .init_resource::<WebTransportServer>()
        .add_systems(Startup, open_server)
        .add_systems(
            PreUpdate,
            (on_opened, on_closed, on_connecting, on_server_event).after(ServerTransportSet::Recv),
        )
        .run();
}

fn open_server(
    mut server: ResMut<WebTransportServer>,
    tasks: Res<TokioRuntime>,
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

fn on_server_event(mut commands: Commands, mut events: EventReader<ServerEvent>) {
    for event in events.read() {
        match event {
            ServerEvent::ClientConnected { client_id } => {
                info!("{client_id:?} connected");
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
            }
        }
    }
}
