//! See [`SteamNetClient`].

use {
    crate::{
        SteamworksClient,
        config::SessionConfig,
        session::{SessionError, SteamNetIo, SteamNetSessionPlugin, entity_to_user_data},
    },
    aeronet_io::{
        IoSystems, SessionEndpoint,
        connection::{DisconnectReason, Disconnected},
    },
    bevy_app::prelude::*,
    bevy_ecs::{prelude::*, system::EntityCommand},
    core::net::SocketAddr,
    steamworks::{
        SteamId, networking_sockets::NetConnection, networking_types::NetworkingIdentity,
    },
    sync_wrapper::SyncWrapper,
};

/// Allows using [`SteamNetClient`].
///
/// This does not perform Steam initialization when the plugin is built;
/// instead, it defers initialization to when a [`SteamNetClient`] is added
/// to the world. This allows you to always add this plugin, but choose at
/// runtime whether you want to use Steam or not.
pub struct SteamNetClientPlugin;

impl Plugin for SteamNetClientPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<SteamNetSessionPlugin>() {
            app.add_plugins(SteamNetSessionPlugin);
        }

        app.add_systems(PreUpdate, poll_connecting.in_set(IoSystems::Poll));
    }
}

/// Steam socket client which can connect to a dedicated server or another peer
/// running a listen server.
///
/// Use [`SteamNetClient::connect`] to start a connection.
#[derive(Debug, Component)]
#[require(SessionEndpoint)]
pub struct SteamNetClient(());

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

impl SteamNetClient {
    /// Creates an [`EntityCommand`] to set up a session and connect it to the
    /// `target`.
    ///
    /// [`SteamworksClient`] must be present in the world before this command is
    /// applied.
    ///
    /// # Examples
    ///
    /// ```
    /// use {
    ///     aeronet_steam::{SessionConfig, client::SteamNetClient},
    ///     bevy_ecs::prelude::*,
    ///     std::net::SocketAddr,
    /// };
    ///
    /// # fn run(mut commands: Commands, world: &mut World) {
    /// let config = SessionConfig::default();
    /// let target = "127.0.0.1:27015".parse::<SocketAddr>().unwrap();
    ///
    /// // using `Commands`
    /// commands
    ///     .spawn_empty()
    ///     .queue(SteamNetClient::connect(config, target));
    ///
    /// // using mutable `World` access
    /// # let config: SessionConfig = unreachable!();
    /// let session = world.spawn_empty().id();
    /// SteamNetClient::connect(config, target).apply(world.entity_mut(session));
    /// # }
    /// ```
    #[must_use]
    pub fn connect(config: SessionConfig, target: impl Into<ConnectTarget>) -> impl EntityCommand {
        let target = target.into();
        move |entity: EntityWorldMut| connect(entity, config, target)
    }
}

fn connect(mut entity: EntityWorldMut, config: SessionConfig, target: ConnectTarget) {
    let mtu = config.send_buffer_size;
    let sockets = entity
        .world()
        .resource::<SteamworksClient>()
        .networking_sockets();
    let (send_next, recv_next) = oneshot::channel::<ConnectResult>();
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
        SteamNetClient(()),
        Connecting {
            recv_next: SyncWrapper::new(recv_next),
            mtu,
        },
    ));
}

type ConnectResult = Result<NetConnection, SessionError>;

#[derive(Component)]
struct Connecting {
    recv_next: SyncWrapper<oneshot::Receiver<ConnectResult>>,
    mtu: usize,
}

fn poll_connecting(
    mut commands: Commands,
    mut clients: Query<(Entity, &mut Connecting), With<SteamNetClient>>,
) {
    for (entity, mut client) in &mut clients {
        let conn = match client.recv_next.get_mut().try_recv() {
            Ok(Ok(conn)) => conn,
            Ok(Err(err)) => {
                commands.trigger(Disconnected {
                    entity,
                    reason: DisconnectReason::by_error(err),
                });
                continue;
            }
            Err(oneshot::TryRecvError::Empty) => continue,
            Err(oneshot::TryRecvError::Disconnected) => {
                commands.trigger(Disconnected {
                    entity,
                    reason: DisconnectReason::by_error(SessionError::BackendClosed),
                });
                continue;
            }
        };

        let user_data = entity_to_user_data(entity);
        if conn.set_connection_user_data(user_data).is_err() {
            commands.trigger(Disconnected {
                entity,
                reason: DisconnectReason::by_error(SessionError::Steam),
            });
            continue;
        }

        commands
            .entity(entity)
            .remove::<Connecting>()
            .insert(SteamNetIo {
                conn,
                mtu: client.mtu,
            });
    }
}
