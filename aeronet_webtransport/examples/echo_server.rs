use aeronet::{
    error::pretty_error,
    lane::{LaneKey, LaneKind},
    server::{ServerEvent, ServerTransport},
};
use aeronet_proto::session::{LaneConfig, SessionConfig};
use aeronet_webtransport::{
    server::{ClientKey, ConnectionResponse, ServerConfig, WebTransportServer},
    wtransport,
};
use bevy::{log::LogPlugin, prelude::*};
use bevy_ecs::system::SystemId;
use bevy_tokio_tasks::{TokioTasksPlugin, TokioTasksRuntime};
use web_time::Duration;

#[derive(Debug, Clone, Copy, LaneKey)]
enum Lane {
    #[lane_kind(ReliableOrdered)]
    Default,
}

fn main() {
    App::new()
        .add_plugins((
            MinimalPlugins,
            LogPlugin::default(),
            TokioTasksPlugin::default(),
        ))
        .init_resource::<WebTransportServer>()
        .add_systems(Startup, setup)
        .add_systems(Update, poll_server)
        .run();
}

fn server_config(identity: &wtransport::Identity) -> ServerConfig {
    wtransport::ServerConfig::builder()
        .with_bind_default(25565)
        .with_identity(&identity)
        .keep_alive_interval(Some(Duration::from_secs(1)))
        .max_idle_timeout(Some(Duration::from_secs(5)))
        .unwrap()
        .build()
}

fn session_config() -> SessionConfig {
    let lanes = vec![LaneConfig::new(LaneKind::ReliableOrdered)];
    SessionConfig {
        send_lanes: lanes.clone(),
        recv_lanes: lanes,
        default_packet_cap: 0,
        max_packet_len: 1024,
        send_bytes_per_sec: usize::MAX,
        max_recv_memory_usage: usize::MAX,
    }
}

fn setup(
    mut commands: Commands,
    mut server: ResMut<WebTransportServer>,
    rt: Res<TokioTasksRuntime>,
) {
    let identity = wtransport::Identity::self_signed(["localhost", "127.0.0.1", "::1"]).unwrap();
    let cert = &identity.certificate_chain().as_slice()[0];
    let spki_fingerprint = aeronet_webtransport::cert::spki_fingerprint_base64(cert).unwrap();
    info!("*** SPKI FINGERPRINT ***");
    info!("{spki_fingerprint}");
    info!("************************");

    let backend = server
        .open(server_config(&identity), session_config())
        .unwrap();
    rt.runtime().spawn(backend);

    let accept_client = commands.register_one_shot_system(accept_client);
    commands.insert_resource(AcceptClient(accept_client));

    let send_message = commands.register_one_shot_system(send_message);
    commands.insert_resource(SendMessage(send_message));
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
    match server.send(client_key, msg.clone(), Lane::Default) {
        Ok(_) => info!("{client_key} < {msg}"),
        Err(err) => warn!(
            "Failed to send message to {client_key}: {:#}",
            pretty_error(&err)
        ),
    }
}
