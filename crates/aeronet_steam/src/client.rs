use std::net::SocketAddr;

use aeronet_io::connection::{DisconnectReason, Disconnected, Session};
use bevy_ecs::{prelude::*, system::EntityCommand};
use steamworks::{
    networking_sockets::InvalidHandle, networking_types::NetworkingIdentity, SteamId,
};
use thiserror::Error;

use crate::session::SteamIo;

#[derive(Debug, Component)]
pub struct SteamClient {}

#[derive(Debug, Clone)]
pub enum ConnectTarget {
    Addr(SocketAddr),
    Peer {
        identity: NetworkingIdentity,
        virtual_port: i32,
    },
}

impl ConnectTarget {
    #[must_use]
    pub fn peer(steam_id: SteamId, virtual_port: i32) -> Self {
        Self::Peer {
            identity: NetworkingIdentity::new_steam_id(steam_id),
            virtual_port,
        }
    }
}

impl From<SocketAddr> for ConnectTarget {
    fn from(value: SocketAddr) -> Self {
        Self::Addr(value)
    }
}

impl From<SteamId> for ConnectTarget {
    fn from(value: SteamId) -> Self {
        Self::peer(value, 0)
    }
}

#[derive(Debug, Clone, Copy, Error)]
#[error("steam error")]
pub struct SteamError;

impl SteamClient {
    #[must_use]
    pub fn connect(target: impl Into<ConnectTarget>) -> impl EntityCommand {
        let target = target.into();
        move |session: Entity, world: &mut World| connect(session, world, target)
    }
}

fn connect(session: Entity, world: &mut World, target: ConnectTarget) {
    world.resource_scope(|world, steam: Mut<bevy_steamworks::Client>| {
        world.entity_mut(session).insert(Session);

        let options = [];
        let result = match target {
            ConnectTarget::Addr(addr) => steam
                .networking_sockets()
                .connect_by_ip_address(addr, options),
            ConnectTarget::Peer {
                identity,
                virtual_port,
            } => steam
                .networking_sockets()
                .connect_p2p(identity, virtual_port, options),
        };

        let conn = match result {
            Ok(conn) => conn,
            Err(InvalidHandle) => {
                world.trigger_targets(
                    Disconnected {
                        reason: DisconnectReason::Error(SteamError.into()),
                    },
                    session,
                );
                return;
            }
        };

        world.entity_mut(session).insert(SteamIo { conn });
    })
}
