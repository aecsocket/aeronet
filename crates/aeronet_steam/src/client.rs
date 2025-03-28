//! See [`SteamNetClient`].

use {
    crate::{
        SteamManager, SteamworksClient,
        config::SessionConfig,
        session::{SessionError, SteamNetIo, SteamNetSessionPlugin, entity_to_user_data},
    },
    aeronet_io::{IoSet, SessionEndpoint, connection::Disconnected},
    bevy_app::prelude::*,
    bevy_ecs::{prelude::*, system::EntityCommand},
    core::{marker::PhantomData, net::SocketAddr},
    steamworks::{
        SteamId, networking_sockets::NetConnection, networking_types::NetworkingIdentity,
    },
    sync_wrapper::SyncWrapper,
};

/// Allows using [`SteamNetClient`].
pub struct SteamNetClientPlugin<M: SteamManager> {
    _phantom: PhantomData<M>,
}

impl<M: SteamManager> Default for SteamNetClientPlugin<M> {
    fn default() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }
}

impl<M: SteamManager> Plugin for SteamNetClientPlugin<M> {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<SteamNetSessionPlugin<M>>() {
            app.add_plugins(SteamNetSessionPlugin::<M>::default());
        }

        app.add_systems(PreUpdate, poll_connecting::<M>.in_set(IoSet::Poll));
    }
}

/// Steam socket client which can connect to a dedicated server or another peer
/// running a listen server.
///
/// Use [`SteamNetClient::connect`] to start a connection.
#[derive(Debug, Component)]
#[require(SessionEndpoint)]
pub struct SteamNetClient<M: SteamManager> {
    _phantom: PhantomData<M>,
}

/// Where a [`SteamNetClient`] will connect to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectTarget {
    /// Connect to a server via a socket address.
    Addr(SocketAddr),
    /// Connect to another Steam user on the Steam relay network in a
    /// peer-to-peer configuration.
    Peer {
        /// ID of the Steam user to connect to.
        steam_id: SteamId,
        /// Steam-specific application port to connect to.
        ///
        /// This acts like [`SocketAddr::port`] but specialized for Steam's P2P
        /// networking.
        virtual_port: i32,
    },
}

impl From<SocketAddr> for ConnectTarget {
    fn from(value: SocketAddr) -> Self {
        Self::Addr(value)
    }
}

impl From<SteamId> for ConnectTarget {
    fn from(steam_id: SteamId) -> Self {
        Self::Peer {
            steam_id,
            virtual_port: 0,
        }
    }
}

impl<M: SteamManager> SteamNetClient<M> {
    /// Creates an [`EntityCommand`] to set up a session and connect it to the
    /// `target`.
    ///
    /// # Examples
    ///
    /// ```
    /// use {
    ///     aeronet_steam::{SessionConfig, client::SteamNetClient},
    ///     bevy_ecs::prelude::*,
    ///     std::net::SocketAddr,
    ///     steamworks::ClientManager,
    /// };
    ///
    /// # fn run(mut commands: Commands, world: &mut World) {
    /// let config = SessionConfig::default();
    /// let target = "127.0.0.1:27015".parse::<SocketAddr>().unwrap();
    ///
    /// // using `Commands`
    /// commands
    ///     .spawn_empty()
    ///     .queue(SteamNetClient::<ClientManager>::connect(config, target));
    ///
    /// // using mutable `World` access
    /// # let config: SessionConfig = unreachable!();
    /// let session = world.spawn_empty().id();
    /// SteamNetClient::<ClientManager>::connect(config, target).apply(world.entity_mut(session));
    /// # }
    /// ```
    #[must_use]
    pub fn connect(config: SessionConfig, target: impl Into<ConnectTarget>) -> impl EntityCommand {
        let target = target.into();
        move |entity: EntityWorldMut| connect::<M>(entity, config, target)
    }
}

fn connect<M: SteamManager>(
    mut entity: EntityWorldMut,
    config: SessionConfig,
    target: ConnectTarget,
) {
    let mtu = config.send_buffer_size;
    let sockets = entity
        .world()
        .resource::<SteamworksClient<M>>()
        .networking_sockets();
    let (send_next, recv_next) = oneshot::channel::<ConnectResult<M>>();
    blocking::unblock(move || {
        let result = match target {
            ConnectTarget::Addr(addr) => sockets.connect_by_ip_address(addr, config.to_options()),
            ConnectTarget::Peer {
                steam_id,
                virtual_port,
            } => sockets.connect_p2p(
                NetworkingIdentity::new_steam_id(steam_id),
                virtual_port,
                config.to_options(),
            ),
        };
        _ = send_next.send(result.map_err(|_| SessionError::Steam));
    })
    .detach();

    entity.insert((
        SteamNetClient::<M> {
            _phantom: PhantomData,
        },
        Connecting {
            recv_next: SyncWrapper::new(recv_next),
            mtu,
        },
    ));
}

type ConnectResult<M> = Result<NetConnection<M>, SessionError>;

#[derive(Component)]
struct Connecting<M> {
    recv_next: SyncWrapper<oneshot::Receiver<ConnectResult<M>>>,
    mtu: usize,
}

fn poll_connecting<M: SteamManager>(
    mut commands: Commands,
    mut clients: Query<(Entity, &mut Connecting<M>), With<SteamNetClient<M>>>,
) {
    for (entity, mut client) in &mut clients {
        let conn = match client.recv_next.get_mut().try_recv() {
            Ok(Ok(conn)) => conn,
            Ok(Err(err)) => {
                commands.trigger_targets(Disconnected::by_error(err), entity);
                continue;
            }
            Err(oneshot::TryRecvError::Empty) => continue,
            Err(oneshot::TryRecvError::Disconnected) => {
                commands
                    .trigger_targets(Disconnected::by_error(SessionError::BackendClosed), entity);
                continue;
            }
        };

        let user_data = entity_to_user_data(entity);
        if conn.set_connection_user_data(user_data).is_err() {
            commands.trigger_targets(Disconnected::by_error(SessionError::Steam), entity);
            continue;
        }

        commands
            .entity(entity)
            .remove::<Connecting<M>>()
            .insert(SteamNetIo {
                conn,
                mtu: client.mtu,
            });
    }
}
