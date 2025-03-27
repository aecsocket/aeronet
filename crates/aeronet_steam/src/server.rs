use {
    crate::{
        SteamManager, SteamworksClient, config::SteamSessionConfig, session::SteamNetSessionPlugin,
    },
    aeronet_io::{
        SessionEndpoint,
        server::{Closed, Server, ServerEndpoint},
    },
    bevy_app::prelude::*,
    bevy_ecs::prelude::*,
    bevy_platform_support::{collections::HashMap, time::Instant},
    core::{marker::PhantomData, net::SocketAddr},
    derive_more::{Display, Error},
    steamworks::{
        networking_sockets::ListenSocket,
        networking_types::{ConnectionRequest, ListenSocketEvent},
    },
    sync_wrapper::SyncWrapper,
};

pub struct SteamNetServerPlugin<M: SteamManager> {
    _phantom: PhantomData<M>,
}

impl<M: SteamManager> Default for SteamNetServerPlugin<M> {
    fn default() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }
}

impl<M: SteamManager> Plugin for SteamNetServerPlugin<M> {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<SteamNetSessionPlugin<M>>() {
            app.add_plugins(SteamNetSessionPlugin::<M>::default());
        }
    }
}

#[derive(Debug, Component)]
#[require(ServerEndpoint)]
pub struct SteamNetServer<M: SteamManager> {
    _phantom: PhantomData<M>,
    clients: HashMap<O>,
}

#[derive(Debug, Component)]
#[require(SessionEndpoint)]
pub struct SteamNetServerClient<M: SteamManager> {
    _phantom: PhantomData<M>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ListenTarget {
    Addr(SocketAddr),
    Peer { virtual_port: i32 },
}

impl From<SocketAddr> for ListenTarget {
    fn from(value: SocketAddr) -> Self {
        Self::Addr(value)
    }
}

impl<M: SteamManager> SteamNetServer<M> {
    #[must_use]
    pub fn open(config: SteamSessionConfig, target: impl Into<ListenTarget>) -> impl EntityCommand {
        let target = target.into();
        move |entity: EntityWorldMut| open::<M>(entity, config, target)
    }
}

fn open<M: SteamManager>(
    mut entity: EntityWorldMut,
    config: SteamSessionConfig,
    target: ListenTarget,
) {
    let (send_next, recv_next) = oneshot::channel::<OpenResult<M>>();
    let sockets = entity
        .world()
        .resource::<SteamworksClient<M>>()
        .networking_sockets();
    blocking::unblock(move || {
        let result = match target {
            ListenTarget::Addr(addr) => sockets.create_listen_socket_ip(addr, config.to_options()),
            ListenTarget::Peer { virtual_port } => {
                sockets.create_listen_socket_p2p(virtual_port, config.to_options())
            }
        };
        _ = send_next.send(result.map_err(|_| ServerError::Steam));
    })
    .detach();

    entity.insert((
        SteamNetServer::<M> {
            _phantom: PhantomData,
        },
        Opening {
            recv_next: SyncWrapper::new(recv_next),
        },
    ));
}

#[derive(Debug, Display, Error)]
#[non_exhaustive]
pub enum ServerError {
    #[display("backend closed")]
    BackendClosed,
    #[display("steam error")]
    Steam,
}

type OpenResult<M> = Result<ListenSocket<M>, ServerError>;

#[derive(Component)]
struct Opening<M> {
    recv_next: SyncWrapper<oneshot::Receiver<OpenResult<M>>>,
}

#[derive(Component)]
struct Opened<M> {
    socket: ListenSocket<M>,
}

#[derive(Component)]
struct Connecting<M> {
    request: ConnectionRequest<M>,
}

fn poll_opening<M: SteamManager>(
    mut commands: Commands,
    mut servers: Query<(Entity, &mut Opening<M>), With<SteamNetServer<M>>>,
) {
    for (entity, mut server) in &mut servers {
        let socket = match server.recv_next.get_mut().try_recv() {
            Ok(Ok(socket)) => socket,
            Ok(Err(err)) => {
                commands.trigger_targets(Closed::by_error(err), entity);
                continue;
            }
            Err(oneshot::TryRecvError::Empty) => continue,
            Err(oneshot::TryRecvError::Disconnected) => {
                commands.trigger_targets(Closed::by_error(ServerError::BackendClosed), entity);
                continue;
            }
        };

        commands
            .entity(entity)
            .remove::<Opening<M>>()
            .insert((Opened { socket }, Server::new(Instant::now())));
    }
}

fn poll_opened<M: SteamManager>(
    mut commands: Commands,
    servers: Query<(Entity, &Opened<M>), With<SteamNetServer<M>>>,
) {
    for (entity, server) in &servers {
        while let Some(event) = server.socket.try_receive_event() {
            match event {
                ListenSocketEvent::Connecting(request) => {
                    commands.spawn((
                        ChildOf { parent: entity },
                        SteamNetServerClient::<M> {
                            _phantom: PhantomData,
                        },
                        Connecting { request },
                    ));
                }
                ListenSocketEvent::Connected(event) => {}
                ListenSocketEvent::Disconnected(event) => {}
            }
        }
    }
}
