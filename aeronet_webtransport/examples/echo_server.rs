//! Example server using WebTransport which listens for clients sending strings
//! and sends back a string reply.

use aeronet::{
    error::pretty_error,
    lane::{LaneIndex, LaneKind},
    server::{server_open, ServerEvent, ServerTransport},
};
use aeronet_proto::session::{LaneConfig, SessionConfig};
use aeronet_webtransport::{
    server::{ClientKey, ConnectionResponse, ServerConfig, WebTransportServer},
    wtransport,
};
use bevy::{log::LogPlugin, prelude::*};
use bevy_ecs::system::SystemId;
use web_time::Duration;

#[derive(Debug, Resource)]
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

#[derive(Debug, Clone, Copy)]
struct AppLane;

impl From<AppLane> for LaneIndex {
    fn from(_: AppLane) -> Self {
        Self::from_raw(0)
    }
}

impl From<AppLane> for LaneConfig {
    fn from(_: AppLane) -> Self {
        Self::new(LaneKind::ReliableOrdered)
    }
}

fn main() {
    App::new()
        .add_plugins((MinimalPlugins, LogPlugin::default()))
        .init_resource::<TokioRuntime>()
        .init_resource::<WebTransportServer>()
        .add_systems(Startup, (setup_one_shot_systems, setup_server).chain())
        .add_systems(PreUpdate, poll_server)
        .add_systems(
            PostUpdate,
            flush_server.run_if(server_open::<WebTransportServer>),
        )
        .run();
}

fn server_config(identity: &wtransport::Identity) -> ServerConfig {
    wtransport::ServerConfig::builder()
        .with_bind_default(25565)
        .with_identity(&identity)
        .max_idle_timeout(Some(Duration::from_secs(5)))
        .unwrap()
        .build()
}

fn session_config() -> SessionConfig {
    // configure both the sending and receiving lanes
    // we will buffer up to 4MB of the server's fragments at once
    // TODO is 4MB a reasonable number?
    SessionConfig::new(1024 * 1024 * 4).with_lanes([AppLane])
}

fn setup_one_shot_systems(world: &mut World) {
    let accept_client = world.register_system(accept_client);
    world.insert_resource(AcceptClient(accept_client));

    let send_message = world.register_system(send_message);
    world.insert_resource(SendMessage(send_message));
}

fn setup_server(mut server: ResMut<WebTransportServer>, rt: Res<TokioRuntime>) {
    let identity = wtransport::Identity::self_signed(["localhost", "127.0.0.1", "::1"]).unwrap();
    let cert = &identity.certificate_chain().as_slice()[0];
    let spki_fingerprint = aeronet_webtransport::cert::spki_fingerprint_base64(cert).unwrap();
    info!("*** SPKI FINGERPRINT ***");
    info!("{spki_fingerprint}");
    info!("************************");

    let backend = server
        .open(server_config(&identity), session_config())
        .unwrap();
    rt.0.spawn(backend);
}

fn poll_server(
    time: Res<Time>,
    mut server: ResMut<WebTransportServer>,
    mut commands: Commands,
    accept_client: Res<AcceptClient>,
    send_message: Res<SendMessage>,
) {
    for event in server.poll(time.delta()) {
        match event {
            ServerEvent::Opened => {
                info!("Server opened");
            }
            ServerEvent::Closed { error } => {
                info!("Server closed: {:#}", pretty_error(&error));
            }
            ServerEvent::Connecting { client_key } => {
                info!("Client {client_key} connecting");
                commands.run_system_with_input(accept_client.0, client_key);
            }
            ServerEvent::Connected { client_key } => {
                info!("Client {client_key} connected");
                commands.run_system_with_input(
                    send_message.0,
                    (
                        client_key,
                        format!("Welcome! Send a message and I will echo it back"),
                    ),
                );
            }
            ServerEvent::Recv {
                client_key, msg, ..
            } => {
                let msg =
                    String::from_utf8(msg.into()).unwrap_or_else(|_| format!("<invalid UTF-8>"));
                info!("{client_key} > {msg}");

                let resp = format!("You sent: {msg}");
                commands.run_system_with_input(send_message.0, (client_key, resp));
            }
            ServerEvent::Disconnected { client_key, error } => {
                info!("{client_key} disconnected: {:#}", pretty_error(&error));
            }
            ServerEvent::Ack { .. } | ServerEvent::Nack { .. } => {}
        }
    }
}

#[derive(Debug, Clone, Resource)]
struct AcceptClient(SystemId<ClientKey>);

fn accept_client(In(client_key): In<ClientKey>, mut server: ResMut<WebTransportServer>) {
    match server.respond_to_request(client_key, ConnectionResponse::Accept) {
        Ok(()) => info!("Accepted {client_key}"),
        Err(err) => warn!("Failed to accept {client_key}: {:#}", pretty_error(&err)),
    }
}

#[derive(Debug, Clone, Resource)]
struct SendMessage(SystemId<(ClientKey, String)>);

fn send_message(
    In((client_key, msg)): In<(ClientKey, String)>,
    mut server: ResMut<WebTransportServer>,
) {
    match server.send(client_key, msg.clone(), AppLane) {
        Ok(_) => info!("{client_key} < {msg}"),
        Err(err) => warn!(
            "Failed to send message to {client_key}: {:#}",
            pretty_error(&err)
        ),
    }
}

fn flush_server(mut server: ResMut<WebTransportServer>) {
    if let Err(err) = server.flush() {
        error!("Failed to flush messages: {:#}", pretty_error(&err));
    }
}
