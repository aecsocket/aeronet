use {
    super::{backend, ConnectionResponse, ServerError, ToConnected, ToConnecting, ToOpen},
    crate::{
        runtime::WebTransportRuntime,
        session::{SessionError, WebTransportIo, WebTransportSessionPlugin},
    },
    aeronet_io::{
        connection::{DisconnectReason, Disconnected, LocalAddr, RemoteAddr, Session},
        packet::{PacketBuffersCapacity, PacketMtu, PacketRtt},
        server::{CloseReason, Closed, Opened, RemoteClient, Server},
        IoSet,
    },
    bevy_app::prelude::*,
    bevy_ecs::{prelude::*, system::EntityCommand},
    bevy_hierarchy::BuildChildren,
    bevy_reflect::prelude::*,
    futures::channel::{mpsc, oneshot},
    std::collections::HashMap,
    tracing::{debug_span, Instrument},
    wtransport::ServerConfig,
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
            .add_systems(PreUpdate, (poll_servers, poll_clients).before(IoSet::Poll))
            .observe(on_server_added)
            .observe(on_connection_response);
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
            let Err(err) = backend::start(runtime, packet_buf_cap, config, send_next).await else {
                unreachable!();
            };
            let _ = send_closed.send(CloseReason::Error(err));
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

#[derive(Debug, Component, Clone, PartialEq, Eq, Reflect)]
#[reflect(Component)]
pub struct WebTransportSessionRequest {
    pub authority: String,
    pub path: String,
    pub origin: Option<String>,
    pub user_agent: Option<String>,
    pub headers: HashMap<String, String>,
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
        send_conn_response: Option<oneshot::Sender<ConnectionResponse>>,
        recv_dc: oneshot::Receiver<DisconnectReason<ServerError>>,
        recv_next: oneshot::Receiver<ToConnected>,
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
                RemoteClient { server },
                WebTransportSessionRequest {
                    authority: connecting.authority,
                    path: connecting.path,
                    origin: connecting.origin,
                    user_agent: connecting.user_agent,
                    headers: connecting.headers,
                },
                ClientFrontend::Connecting {
                    send_conn_response: Some(connecting.send_conn_response),
                    recv_dc: connecting.recv_dc,
                    recv_next: connecting.recv_next,
                },
            ))
            .set_parent(server)
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

    let _ = sender.send(*trigger.event());
}

fn poll_clients(mut commands: Commands, mut clients: Query<(Entity, &mut ClientFrontend)>) {
    for (client, mut frontend) in &mut clients {
        replace_with::replace_with_or_abort(&mut *frontend, |state| match state {
            ClientFrontend::Connecting {
                send_conn_response,
                recv_dc,
                recv_next,
            } => poll_connecting(
                &mut commands,
                client,
                send_conn_response,
                recv_dc,
                recv_next,
            ),
            ClientFrontend::Connected { mut recv_dc } => {
                if should_disconnect(&mut commands, client, &mut recv_dc) {
                    ClientFrontend::Disconnected
                } else {
                    ClientFrontend::Connected { recv_dc }
                }
            }
            ClientFrontend::Disconnected => state,
        });
    }
}

fn poll_connecting(
    commands: &mut Commands,
    client: Entity,
    send_conn_response: Option<oneshot::Sender<ConnectionResponse>>,
    mut recv_dc: oneshot::Receiver<DisconnectReason<ServerError>>,
    mut recv_next: oneshot::Receiver<ToConnected>,
) -> ClientFrontend {
    if should_disconnect(commands, client, &mut recv_dc) {
        return ClientFrontend::Disconnected;
    }

    let Ok(Some(next)) = recv_next.try_recv() else {
        return ClientFrontend::Connecting {
            send_conn_response,
            recv_dc,
            recv_next,
        };
    };

    commands.entity(client).insert((
        WebTransportIo {
            recv_meta: next.recv_meta,
            recv_packet_b2f: next.recv_packet_b2f,
            send_packet_f2b: next.send_packet_f2b,
            send_user_dc: Some(next.send_user_dc),
        },
        PacketMtu(next.initial_mtu),
        RemoteAddr(next.initial_remote_addr),
        PacketRtt(next.initial_rtt),
    ));
    ClientFrontend::Connected { recv_dc }
}

fn should_disconnect(
    commands: &mut Commands,
    client: Entity,
    recv_dc: &mut oneshot::Receiver<DisconnectReason<ServerError>>,
) -> bool {
    let dc_reason = match recv_dc.try_recv() {
        Ok(None) => None,
        Ok(Some(dc_reason)) => Some(dc_reason),
        Err(_) => Some(ServerError::Session(SessionError::BackendClosed).into()),
    };
    if let Some(reason) = dc_reason {
        let reason = reason.map_err(anyhow::Error::new);
        commands.trigger_targets(Disconnected { reason }, client);
        true
    } else {
        false
    }
}
