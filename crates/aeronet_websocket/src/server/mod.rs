use std::{io, net::SocketAddr};

use aeronet_io::{
    connection::DisconnectReason,
    packet::PacketBuffersCapacity,
    server::{CloseReason, Server},
    IoSet,
};
use bevy_app::prelude::*;
use bevy_ecs::{prelude::*, system::EntityCommand};
use futures::{
    channel::{mpsc, oneshot},
    never::Never,
    SinkExt, TryFutureExt,
};
use thiserror::Error;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{tungstenite::protocol::WebSocketConfig, MaybeTlsStream};
use tracing::{debug, debug_span, Instrument};

use crate::{
    session::{self, SessionError, SessionFrontend, WebSocketSessionPlugin},
    tungstenite, WebSocketRuntime,
};

#[derive(Debug)]
pub struct WebSocketServerPlugin;

impl Plugin for WebSocketServerPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<WebSocketSessionPlugin>() {
            app.add_plugins(WebSocketSessionPlugin);
        }

        app.add_systems(
            PreUpdate,
            (poll_servers, poll_clients)
                .in_set(IoSet::Poll)
                .before(session::poll),
        )
        .observe(on_server_added);
    }
}

#[derive(Debug, Component)]
pub struct WebSocketServer(Frontend);

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub addr: SocketAddr,
    pub socket: WebSocketConfig,
}

#[derive(Debug, Error)]
pub enum ServerError {
    #[error("failed to bind socket")]
    BindSocket(#[source] io::Error),
    #[error("failed to accept connection")]
    AcceptConnection(#[source] io::Error),
    #[error("failed to accept client")]
    AcceptClient(#[source] tungstenite::Error),
    #[error(transparent)]
    Session(#[from] SessionError),
}

impl WebSocketServer {
    #[must_use]
    pub fn open(config: ServerConfig) -> impl EntityCommand {
        move |server: Entity, world: &mut World| open(server, world, config)
    }
}

fn open(server: Entity, world: &mut World, config: ServerConfig) {
    let runtime = world.resource::<WebSocketRuntime>().clone();
    let packet_buf_cap = PacketBuffersCapacity::compute_from(world, server);

    let (send_closed, recv_closed) = oneshot::channel::<CloseReason<ServerError>>();
    let (send_next, recv_next) = oneshot::channel::<ToOpen>();
    runtime.spawn_on_self(
        async move {
            let Err(err) = start(config, packet_buf_cap, send_next).await else {
                unreachable!();
            };
            let _ = send_closed.send(CloseReason::Error(err));
        }
        .instrument(debug_span!("server", %server)),
    );

    world
        .entity_mut(server)
        .insert(WebSocketServer(Frontend::Opening {
            recv_closed,
            recv_next,
        }));
}

#[derive(Debug)]
struct ToOpen {
    local_addr: SocketAddr,
    recv_connecting: mpsc::Receiver<ToConnecting>,
}

#[derive(Debug)]
struct ToConnecting {
    remote_addr: SocketAddr,
    send_session_entity: oneshot::Sender<Entity>,
    recv_dc: oneshot::Receiver<DisconnectReason<ServerError>>,
    recv_next: oneshot::Receiver<SessionFrontend>,
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

fn on_server_added(trigger: Trigger<OnAdd, WebSocketServer>, mut commands: Commands) {
    let server = trigger.entity();
    commands.entity(server).insert(Server);
}

fn poll_servers(mut commands: Commands, mut servers: Query<(Entity, &mut WebSocketServer)>) {
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
}

async fn start(
    config: ServerConfig,
    packet_buf_cap: usize,
    send_next: oneshot::Sender<ToOpen>,
) -> Result<Never, ServerError> {
    let listener = TcpListener::bind(config.addr)
        .await
        .map_err(ServerError::BindSocket)?;
    debug!("Listening on {}", config.addr);

    let (send_connecting, recv_connecting) = mpsc::channel::<ToConnecting>(1);

    let local_addr = listener.local_addr().map_err(SessionError::GetLocalAddr)?;
    let next = ToOpen {
        local_addr,
        recv_connecting,
    };
    send_next
        .send(next)
        .map_err(|_| SessionError::FrontendClosed)?;

    debug!("Starting server loop");
    loop {
        let (stream, remote_addr) = listener
            .accept()
            .await
            .map_err(ServerError::AcceptConnection)?;
        tokio::spawn({
            let send_connecting = send_connecting.clone();
            async move {
                if let Err(err) = accept_session(
                    stream,
                    remote_addr,
                    config.socket.clone(),
                    packet_buf_cap,
                    send_connecting,
                )
                .await
                {
                    debug!("Failed to accept session: {err:?}");
                }
            }
        });
    }
}

async fn accept_session(
    stream: TcpStream,
    remote_addr: SocketAddr,
    socket_config: WebSocketConfig,
    packet_buf_cap: usize,
    mut send_connecting: mpsc::Sender<ToConnecting>,
) -> Result<(), DisconnectReason<ServerError>> {
    let (send_session_entity, recv_session_entity) = oneshot::channel::<Entity>();
    let (send_dc, recv_dc) = oneshot::channel::<DisconnectReason<ServerError>>();
    let (send_next, recv_next) = oneshot::channel::<SessionFrontend>();
    send_connecting
        .send(ToConnecting {
            remote_addr,
            send_session_entity,
            recv_dc,
            recv_next,
        })
        .await
        .map_err(|_| SessionError::FrontendClosed)
        .map_err(ServerError::Session)?;
    let session = recv_session_entity
        .await
        .map_err(|_| SessionError::FrontendClosed)
        .map_err(ServerError::Session)?;

    let Err(dc_reason) = handle_session(stream, socket_config, packet_buf_cap, send_next)
        .instrument(debug_span!("session", %session))
        .await
    else {
        unreachable!();
    };
    let _ = send_dc.send(dc_reason);
    Ok(())
}

async fn handle_session(
    stream: TcpStream,
    socket_config: WebSocketConfig,
    packet_buf_cap: usize,
    send_next: oneshot::Sender<SessionFrontend>,
) -> Result<Never, DisconnectReason<ServerError>> {
    // TODO TLS
    // TODO accept hdr: find some way to pass control of headers over to user
    let stream = MaybeTlsStream::Plain(stream);
    let stream = tokio_tungstenite::accept_async_with_config(stream, Some(socket_config))
        .await
        .map_err(ServerError::AcceptClient)?;
    let (frontend, backend) = crate::session::backend::native::split(stream, packet_buf_cap)
        .map_err(ServerError::Session)?;
    debug!("Connected");

    send_next
        .send(frontend)
        .map_err(|_| SessionError::FrontendClosed)
        .map_err(ServerError::Session)?;

    debug!("Starting session loop");
    backend
        .start()
        .await
        .map_err(|reason| reason.map_err(ServerError::Session))
}
