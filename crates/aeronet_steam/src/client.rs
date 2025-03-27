use {
    crate::{
        SteamworksClient,
        config::SteamSessionConfig,
        session::{SteamNetIo, SteamNetSessionPlugin},
    },
    aeronet_io::{IoSet, SessionEndpoint, connection::Disconnected},
    bevy_app::prelude::*,
    bevy_ecs::{prelude::*, system::EntityCommand},
    core::net::SocketAddr,
    derive_more::{Display, Error},
    steamworks::{
        ClientManager, SteamId, networking_sockets::NetConnection,
        networking_types::NetworkingIdentity,
    },
    sync_wrapper::SyncWrapper,
};

/// Allows using [`SteamNetClient`].
#[derive(Debug)]
pub struct SteamNetClientPlugin;

impl Plugin for SteamNetClientPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<SteamNetSessionPlugin>() {
            app.add_plugins(SteamNetSessionPlugin);
        }

        app.add_systems(PreUpdate, poll_connecting.in_set(IoSet::Poll));
    }
}

#[derive(Debug, Component)]
#[require(SessionEndpoint)]
pub struct SteamNetClient(());

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectTarget {
    Addr(SocketAddr),
    Peer {
        steam_id: SteamId,
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
    #[must_use]
    pub fn connect(
        config: SteamSessionConfig,
        target: impl Into<ConnectTarget>,
    ) -> impl EntityCommand {
        let target = target.into();
        move |entity: EntityWorldMut| connect(entity, config, target)
    }
}

fn connect(mut entity: EntityWorldMut, config: SteamSessionConfig, target: ConnectTarget) {
    let steam = entity.world().resource::<SteamworksClient>().clone();

    let mtu = config.send_buffer_size;
    let (send_next, recv_next) = oneshot::channel::<ConnectResult>();
    blocking::unblock(move || {
        let result = match target {
            ConnectTarget::Addr(addr) => steam
                .networking_sockets()
                .connect_by_ip_address(addr, config.to_options()),
            ConnectTarget::Peer {
                steam_id,
                virtual_port,
            } => steam.networking_sockets().connect_p2p(
                NetworkingIdentity::new_steam_id(steam_id),
                virtual_port,
                config.to_options(),
            ),
        };

        _ = send_next.send(result.map_err(|_| ClientError::Steam));
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

#[derive(Debug, Display, Error)]
#[non_exhaustive]
pub enum ClientError {
    #[display("backend closed")]
    BackendClosed,
    #[display("steam error")]
    Steam,
}

#[derive(Component)]
struct Connecting {
    recv_next: SyncWrapper<oneshot::Receiver<ConnectResult>>,
    mtu: usize,
}

type ConnectResult = Result<NetConnection<ClientManager>, ClientError>;

fn poll_connecting(
    mut commands: Commands,
    mut clients: Query<(Entity, &mut Connecting), With<SteamNetClient>>,
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
                    .trigger_targets(Disconnected::by_error(ClientError::BackendClosed), entity);
                continue;
            }
        };

        #[expect(
            clippy::cast_possible_wrap,
            reason = "we treat the entity as an opaque identifier"
        )]
        let user_data = entity.to_bits() as i64;
        if conn.set_connection_user_data(user_data).is_err() {
            commands.trigger_targets(Disconnected::by_error(ClientError::Steam), entity);
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
