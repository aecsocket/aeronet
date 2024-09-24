mod backend;

use std::{io, net::SocketAddr};

use aeronet_io::{
    connection::{DisconnectReason, LocalAddr, Session},
    packet::PacketBuffersCapacity,
    server::{CloseReason, Closed, Opened, RemoteClient, Server},
    IoSet,
};
use bevy_app::prelude::*;
use bevy_ecs::{prelude::*, system::EntityCommand};
use bevy_hierarchy::BuildChildren;
use bytes::Bytes;
use futures::channel::{mpsc, oneshot};
use thiserror::Error;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tracing::{debug_span, Instrument};

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
            let Err(err) = backend::start(config, packet_buf_cap, send_next).await else {
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

#[derive(Debug, Component)]
enum ClientFrontend {
    Connecting {
        recv_dc: oneshot::Receiver<DisconnectReason<ServerError>>,
        recv_next: oneshot::Receiver<SessionFrontend>,
    },
    Connected {
        recv_dc: oneshot::Receiver<DisconnectReason<ServerError>>,
    },
    Disconnected,
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

// TODO: required components
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
            // spawn -> parent -> insert, so that Parent is available
            // as soon as other components are added
            .spawn_empty()
            .set_parent(server)
            .insert((
                Session,
                ClientFrontend::Connecting {
                    recv_dc: connecting.recv_dc,
                    recv_next: connecting.recv_next,
                }
            ))
            .id();
        let _ = connecting.send_session_entity.send(session);
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
    close_reason.map_or(false, |reason| {
        let reason = reason.map_err(anyhow::Error::new);
        commands.trigger_targets(Closed { reason }, server);
        true
    })
}
