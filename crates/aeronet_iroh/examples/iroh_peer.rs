//! Example showing symmetric Iroh peers which print their endpoint IDs and
//! exchange UTF-8 strings.

use {
    aeronet_io::{
        Session, SessionEndpoint,
        connection::{DisconnectReason, Disconnected},
    },
    aeronet_iroh::{
        IrohPlugin,
        endpoint::{EndpointClosed, IrohEndpoint},
        session::{IrohSession, SessionRequest, SessionResponse, SessionSide},
    },
    bevy::{log::LogPlugin, prelude::*},
    clap::Parser,
    iroh::EndpointId,
};

const ALPN: &[u8] = b"aeronet-iroh/example/0";

fn main() -> AppExit {
    App::new()
        .add_plugins((MinimalPlugins, LogPlugin::default(), IrohPlugin))
        .insert_resource(Args::parse())
        .add_systems(Startup, open_endpoint)
        .add_systems(Update, reply)
        .add_observer(on_endpoint_opened)
        .add_observer(on_endpoint_closed)
        .add_observer(on_session_request)
        .add_observer(on_connecting)
        .add_observer(on_connected)
        .add_observer(on_disconnected)
        .run()
}

#[derive(Debug, Parser, Resource)]
#[command(about = "Open an Iroh endpoint and optionally connect to another peer")]
struct Args {
    /// Endpoint ID to connect to.
    #[arg(long, value_name = "ENDPOINT")]
    remote: Option<EndpointId>,
}

fn open_endpoint(mut commands: Commands) {
    let builder = iroh::Endpoint::builder(iroh::endpoint::presets::N0).alpns(vec![ALPN.to_vec()]);
    commands.spawn_empty().queue(IrohEndpoint::open(builder));
}

fn on_endpoint_opened(
    trigger: On<Add, IrohEndpoint>,
    endpoints: Query<&IrohEndpoint>,
    args: Res<Args>,
    mut commands: Commands,
) {
    let entity = trigger.event_target();
    let endpoint = endpoints
        .get(entity)
        .expect("triggered endpoint should exist");

    info!("Local endpoint: {}", endpoint.id());

    if let Some(remote) = args.remote {
        info!("Connecting to {remote}");
        commands.spawn_empty().queue(endpoint.connect(remote, ALPN));
    }
}

fn on_endpoint_closed(trigger: On<EndpointClosed>) {
    panic!("endpoint closed: {:?}", trigger.error);
}

fn on_session_request(mut request: On<SessionRequest>) {
    info!("Accepting session from {}", request.peer_id);
    request.respond(SessionResponse::Accepted);
}

fn on_connecting(trigger: On<Add, SessionEndpoint>, sessions: Query<&IrohSession>) {
    let entity = trigger.event_target();
    let Ok(session) = sessions.get(entity) else {
        return;
    };

    info!("{entity} connecting to {}", session.peer_id());
}

fn on_connected(
    trigger: On<Add, Session>,
    endpoints: Query<&IrohEndpoint>,
    mut sessions: Query<(&IrohSession, &mut Session)>,
) {
    let entity = trigger.event_target();
    let (iroh_session, mut session) = sessions
        .get_mut(entity)
        .expect("connected Iroh session should exist");
    let peer = iroh_session.peer_id();
    info!("{entity} connected to {peer}");

    if iroh_session.side() == SessionSide::Outgoing {
        let endpoint = endpoints
            .get(iroh_session.endpoint())
            .expect("session endpoint should exist");
        let message = format!("Hello from {}", endpoint.id());
        info!("{peer} < {message}");
        session.send.push(message.into());
    }
}

fn on_disconnected(trigger: On<Disconnected>, sessions: Query<&IrohSession>) {
    let entity = trigger.event_target();
    let Ok(session) = sessions.get(entity) else {
        return;
    };
    let peer = session.peer_id();

    match &trigger.reason {
        DisconnectReason::ByUser(reason) => {
            info!("{entity} disconnected from {peer} by user: {reason}");
        }
        DisconnectReason::ByPeer(reason) => {
            info!("{entity} disconnected from {peer} by peer: {reason}");
        }
        DisconnectReason::ByError(error) => {
            warn!("{entity} disconnected from {peer} due to error: {error:#}");
        }
    }
}

fn reply(endpoints: Query<&IrohEndpoint>, mut sessions: Query<(&IrohSession, &mut Session)>) {
    for (iroh_session, mut session) in &mut sessions {
        let peer = iroh_session.peer_id();
        let side = iroh_session.side();
        let endpoint = endpoints
            .get(iroh_session.endpoint())
            .expect("session endpoint should exist");

        // Explicit dereference lets us access the disjoint receive and send fields.
        let session = &mut *session;
        for packet in session.recv.drain(..) {
            let message =
                String::from_utf8(packet.payload.into()).unwrap_or_else(|_| "(not UTF-8)".into());
            info!("{peer} > {message}");

            if side == SessionSide::Incoming {
                let response = format!("Hello from {}", endpoint.id());
                info!("{peer} < {response}");
                session.send.push(response.into());
            }
        }
    }
}
