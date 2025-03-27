use std::net::SocketAddr;

use aeronet_io::{
    Session,
    connection::{DisconnectReason, Disconnected},
};
use bevy_ecs::{prelude::*, system::EntityCommand};
use bevy_platform_support::time::Instant;
use derive_more::{Display, Error};
use futures::channel::oneshot;
use steamworks::{
    ClientManager, SteamId, networking_sockets::NetConnection, networking_types::NetworkingIdentity,
};

use crate::{SteamworksClient, config::SteamSessionConfig};

#[derive(Component)]
pub struct SteamClient {
    recv_connect_result: Option<oneshot::Receiver<ConnectResult>>,
}

type ConnectResult = Result<NetConnection<ClientManager>, SteamError>;

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

#[derive(Debug, Clone, Copy, Display, Error)]
#[display("steam error")]
pub struct SteamError;

impl SteamClient {
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
    let (send_connect_result, recv_connect_result) = oneshot::channel::<ConnectResult>();
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

        _ = send_connect_result.send(result.map_err(|_| SteamError));
    })
    .detach();

    entity.insert((
        SteamClient {
            recv_connect_result: Some(recv_connect_result),
        },
        Session::new(Instant::now(), mtu),
    ));
}

#[derive(Debug, Display, Error)]
#[non_exhaustive]
pub enum ClientError {
    #[display("backend closed")]
    BackendClosed,
}

fn poll_clients(mut commands: Commands, mut frontends: Query<(Entity, &mut SteamClient)>) {
    for (entity, mut frontend) in &mut frontends {
        if let Some(recv_connect_result) = &mut frontend.recv_connect_result {
            match recv_connect_result.try_recv() {
                Ok(Some(result)) => {
                    todo!()
                }
                Ok(None) => {}
                Err(oneshot::Canceled) => {
                    commands.trigger_targets(
                        Disconnected {
                            reason: DisconnectReason::Error(ClientError::BackendClosed.into()),
                        },
                        entity,
                    );
                }
            }
        }
    }
}

fn poll_client(entity: Entity, frontend: &mut SteamClient) -> Result<(), ClientError> {
    let Some(recv_connect_result) = frontend.recv_connect_result else {
        return Ok(());
    };
}
