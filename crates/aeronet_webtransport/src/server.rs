use {
    crate::{
        runtime::WebTransportRuntime,
        session::{SessionBackend, SessionError, SessionMeta, WebTransportSessionPlugin},
    },
    aeronet_io::{
        connection::{DisconnectReason, LocalAddr, Session},
        packet::PacketBuffersCapacity,
        server::{CloseReason, Closed, Opened, Server},
        IoSet,
    },
    bevy_app::prelude::*,
    bevy_ecs::{prelude::*, system::EntityCommand},
    bevy_hierarchy::BuildChildren,
    bevy_reflect::prelude::*,
    bytes::Bytes,
    futures::{
        channel::{mpsc, oneshot},
        never::Never,
        SinkExt,
    },
    std::{collections::HashMap, net::SocketAddr, time::Duration},
    thiserror::Error,
    tracing::{debug, debug_span, Instrument},
    wtransport::{
        endpoint::{IncomingSession, SessionRequest},
        error::ConnectionError,
        Endpoint, ServerConfig,
    },
    xwt_core::prelude::*,
};

/// Allows using [`WebTransportServer`].
#[derive(Debug)]
pub struct WebTransportServerPlugin;

impl Plugin for WebTransportServerPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<WebTransportSessionPlugin>() {
            app.add_plugins(WebTransportSessionPlugin);
        }

        app.register_type::<WebTransportSessionRequest>()
            .register_type::<ConnectionResponse>()
            .add_systems(PreUpdate, poll_servers.before(IoSet::Poll))
            .observe(on_server_added);
    }
}

#[derive(Debug, Component)]
pub struct WebTransportServer(Frontend);

impl WebTransportServer {
    #[must_use]
    pub fn open(config: ServerConfig) -> impl EntityCommand {
        |server: Entity, world: &mut World| open(server, world, config)
    }
}

fn open(server: Entity, world: &mut World, config: ServerConfig) {
    let runtime = world.resource::<WebTransportRuntime>().clone();
    let packet_buf_cap = PacketBuffersCapacity::compute_from(world, server);

    let (send_closed, recv_closed) = oneshot::channel::<CloseReason<ServerError>>();
    let (send_next, recv_next) = oneshot::channel::<ToOpen>();
    runtime.spawn({
        let runtime = runtime.clone();
        async move {
            let Err(reason) = backend(runtime, packet_buf_cap, config, send_next).await else {
                unreachable!();
            };
            let _ = send_closed.send(reason);
        }
        .instrument(debug_span!("server", %server))
    });

    world
        .entity_mut(server)
        .insert(WebTransportServer(Frontend::Opening {
            recv_closed,
            recv_next,
        }));
}

#[derive(Debug, Component, Reflect)]
#[reflect(Component)]
pub struct WebTransportSessionRequest {
    pub authority: String,
    pub path: String,
    pub origin: Option<String>,
    pub user_agent: Option<String>,
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Event, Reflect)]
pub enum ConnectionResponse {
    Accepted,
    Forbidden,
    NotFound,
}

/// [`WebTransportServer`] error.
#[derive(Debug, Error)]
pub enum ServerError {
    /// Failed to await an incoming session request.
    #[error("failed to await session request")]
    AwaitSessionRequest(#[source] ConnectionError),
    /// User rejected this incoming session request.
    #[error("user rejected session request")]
    Rejected,
    /// Failed to accept the incoming session request.
    #[error("failed to accept session")]
    AcceptSessionRequest(#[source] ConnectionError),
    /// Generic session error.
    #[error(transparent)]
    Session(#[from] SessionError),
}

#[derive(Debug, Component)]
enum Frontend {
    Opening {
        recv_closed: oneshot::Receiver<CloseReason<ServerError>>,
        recv_next: oneshot::Receiver<ToOpen>,
    },
    Open {
        recv_closed: oneshot::Receiver<CloseReason<ServerError>>,
        recv_connecting: mpsc::Receiver<ToConnecting>,
    },
    Closed,
}

#[derive(Debug)]
struct ToOpen {
    local_addr: SocketAddr,
    recv_connecting: mpsc::Receiver<ToConnecting>,
}

#[derive(Debug)]
struct ToConnecting {
    authority: String,
    path: String,
    origin: Option<String>,
    user_agent: Option<String>,
    headers: HashMap<String, String>,
    send_session_entity: oneshot::Sender<Entity>,
    send_conn_response: oneshot::Sender<ConnectionResponse>,
    recv_connected: oneshot::Receiver<ToConnected>,
}

#[derive(Debug)]
struct ToConnected {
    initial_remote_addr: SocketAddr,
    initial_rtt: Duration,
    initial_mtu: usize,
    recv_meta: mpsc::Receiver<SessionMeta>,
    recv_packet_b2f: mpsc::Receiver<Bytes>,
    send_packet_f2b: mpsc::UnboundedSender<Bytes>,
    send_user_dc: oneshot::Sender<String>,
}

#[derive(Debug, Component)]
enum ClientFrontend {
    Connecting {
        send_conn_response: Option<oneshot::Sender<ConnectionResponse>>,
        recv_connected: oneshot::Receiver<ToConnected>,
    },
    Connected {
        recv_dc: oneshot::Receiver<DisconnectReason<ServerError>>,
    },
    Disconnected,
}

// TODO: required components
fn on_server_added(trigger: Trigger<OnAdd, WebTransportServer>, mut commands: Commands) {
    let server = trigger.entity();
    commands.entity(server).insert(Server);
}

fn poll_servers(mut commands: Commands, mut servers: Query<(Entity, &mut WebTransportServer)>) {
    for (server, mut frontend) in &mut servers {
        replace_with::replace_with_or_abort(&mut frontend.0, |state| match state {
            Frontend::Opening {
                recv_closed,
                recv_next,
            } => poll_opening(&mut commands, server, recv_closed, recv_next),
            Frontend::Open {
                recv_closed,
                recv_connecting,
            } => poll_open(&mut commands, server, recv_closed, recv_connecting),
            Frontend::Closed => state,
        });
    }
}

fn poll_opening(
    commands: &mut Commands,
    server: Entity,
    mut recv_closed: oneshot::Receiver<CloseReason<ServerError>>,
    mut recv_next: oneshot::Receiver<ToOpen>,
) -> Frontend {
    if should_close(commands, server, &mut recv_closed) {
        return Frontend::Closed;
    }

    let Ok(Some(next)) = recv_next.try_recv() else {
        return Frontend::Opening {
            recv_closed,
            recv_next,
        };
    };

    commands
        .entity(server)
        .insert((Opened, LocalAddr(next.local_addr)));
    Frontend::Open {
        recv_closed,
        recv_connecting: next.recv_connecting,
    }
}

fn poll_open(
    commands: &mut Commands,
    server: Entity,
    mut recv_closed: oneshot::Receiver<CloseReason<ServerError>>,
    mut recv_connecting: mpsc::Receiver<ToConnecting>,
) -> Frontend {
    if should_close(commands, server, &mut recv_closed) {
        return Frontend::Closed;
    }

    while let Ok(Some(connecting)) = recv_connecting.try_next() {
        let session = commands
            .spawn((
                Session,
                WebTransportSessionRequest {
                    authority: connecting.authority,
                    path: connecting.path,
                    origin: connecting.origin,
                    user_agent: connecting.user_agent,
                    headers: connecting.headers,
                },
                ClientFrontend::Connecting {
                    send_conn_response: connecting.send_conn_response,
                    recv_connected: connecting.recv_connected,
                },
            ))
            .set_parent(server)
            .id();
        connecting.send_session_entity.send(session);
    }

    Frontend::Open {
        recv_closed,
        recv_connecting,
    }
}

fn should_close(
    commands: &mut Commands,
    server: Entity,
    recv_closed: &mut oneshot::Receiver<CloseReason<ServerError>>,
) -> bool {
    let close_reason = match recv_closed.try_recv() {
        Ok(None) => None,
        Ok(Some(close_reason)) => Some(close_reason),
        Err(_) => Some(ServerError::Session(SessionError::BackendClosed).into()),
    };
    if let Some(reason) = close_reason {
        let reason = reason.map_err(anyhow::Error::new);
        commands.trigger_targets(Closed { reason }, server);
        true
    } else {
        false
    }
}

fn on_connection_response(
    trigger: Trigger<ConnectionResponse>,
    mut clients: Query<&mut ClientFrontend>,
) {
    let client = trigger.entity();
    let Ok(mut frontend) = clients.get_mut(client) else {
        return;
    };
    let ClientFrontend::Connecting {
        send_conn_response, ..
    } = frontend.as_mut()
    else {
        return;
    };
    let Some(sender) = send_conn_response.take() else {
        return;
    };

    sender.send(*trigger.event());
}

fn poll_clients(mut commands: Commands, mut clients: Query<&mut ClientFrontend>) {
    for client in &mut clients {}
}

async fn backend(
    runtime: WebTransportRuntime,
    packet_buf_cap: usize,
    config: ServerConfig,
    send_next: oneshot::Sender<ToOpen>,
) -> Result<Never, ServerError> {
    debug!("Spawning backend task to open server");

    let endpoint = Endpoint::server(config).map_err(SessionError::CreateEndpoint)?;
    debug!("Created endpoint");

    let (send_connecting, recv_connecting) = mpsc::channel(1);

    let next = ToOpen {
        local_addr: endpoint.local_addr().map_err(SessionError::GetLocalAddr)?,
        recv_connecting,
    };
    send_next
        .send(next)
        .map_err(|_| SessionError::FrontendClosed)?;

    debug!("Starting server loop");
    loop {
        let session = endpoint.accept().await;

        runtime.spawn({
            let runtime = runtime.clone();
            let send_connecting = send_connecting.clone();
            async move {
                if let Err(err) =
                    accept_session(runtime, packet_buf_cap, session, send_connecting).await
                {
                    debug!("Failed to accept session: {err:?}");
                };
            }
        });
    }
}

async fn accept_session(
    runtime: WebTransportRuntime,
    packet_buf_cap: usize,
    session: IncomingSession,
    mut send_connecting: mpsc::Sender<ToConnecting>,
) -> Result<(), ServerError> {
    let request = session.await.map_err(ServerError::AwaitSessionRequest)?;

    let (send_session_entity, recv_session_entity) = oneshot::channel::<Entity>();
    let (send_conn_response, recv_conn_response) = oneshot::channel::<ConnectionResponse>();
    let (send_connected, recv_connected) = oneshot::channel::<ToConnected>();
    send_connecting
        .send(ToConnecting {
            authority: request.authority().to_owned(),
            path: request.path().to_owned(),
            origin: request.origin().map(ToOwned::to_owned),
            user_agent: request.user_agent().map(ToOwned::to_owned),
            headers: request.headers().clone(),
            send_session_entity,
            send_conn_response,
            recv_connected,
        })
        .await
        .map_err(|_| SessionError::FrontendClosed)?;
    let session = recv_session_entity
        .await
        .map_err(|_| SessionError::FrontendClosed)?;

    let err = async move {
        let Err(err) = handle_session(
            runtime,
            packet_buf_cap,
            request,
            recv_conn_response,
            send_connected,
        )
        .await
        else {
            unreachable!()
        };
        err
    }
    .instrument(debug_span!("session", session = %session))
    .await;
    Ok(())
}

async fn handle_session(
    runtime: WebTransportRuntime,
    packet_buf_cap: usize,
    request: SessionRequest,
    recv_conn_response: oneshot::Receiver<ConnectionResponse>,
    send_connected: oneshot::Sender<ToConnected>,
) -> Result<Never, DisconnectReason<ServerError>> {
    debug!(
        "New session request from {}{}",
        request.authority(),
        request.path()
    );

    let conn_response = recv_conn_response
        .await
        .map_err(|_| SessionError::FrontendClosed.into())
        .map_err(ServerError::Session)?;
    debug!("Frontend responded to this request with {conn_response:?}");

    let conn = match conn_response {
        ConnectionResponse::Accepted => request.accept(),
        ConnectionResponse::Forbidden => {
            request.forbidden().await;
            return Err(ServerError::Rejected.into());
        }
        ConnectionResponse::NotFound => {
            request.not_found().await;
            return Err(ServerError::Rejected.into());
        }
    }
    .await
    .map(xwt_wtransport::Connection)
    .map_err(ServerError::AcceptSessionRequest)?;
    debug!("Connected");

    let (send_meta, recv_meta) = mpsc::channel::<SessionMeta>(1);
    let (send_packet_b2f, recv_packet_b2f) = mpsc::channel::<Bytes>(packet_buf_cap);
    let (send_packet_f2b, recv_packet_f2b) = mpsc::unbounded::<Bytes>();
    let (send_user_dc, recv_user_dc) = oneshot::channel::<String>();
    let next = ToConnected {
        initial_remote_addr: conn.0.remote_address(),
        initial_rtt: conn.0.rtt(),
        initial_mtu: conn
            .max_datagram_size()
            .ok_or(SessionError::DatagramsNotSupported.into())
            .map_err(ServerError::Session)?,
        recv_meta,
        recv_packet_b2f,
        send_packet_f2b,
        send_user_dc,
    };
    let backend = SessionBackend {
        runtime,
        conn,
        send_meta,
        send_packet_b2f,
        recv_packet_f2b,
        recv_user_dc,
    };
    send_connected
        .send(next)
        .map_err(|_| SessionError::FrontendClosed)
        .map_err(ServerError::Session)?;

    debug!("Starting session loop");
    Err(backend.start().await.map_err(ServerError::Session))
}
