use std::net::SocketAddr;

use aeronet_io::connection::{DisconnectReason, Disconnected};
use bevy_ecs::{prelude::*, system::EntityCommand};
use derive_more::{Display, Error};
use steamworks::{
    SteamId, networking_sockets::InvalidHandle, networking_types::NetworkingIdentity,
};

use crate::{config::SteamSessionConfig, session::SteamIo};

#[derive(Debug, Component)]
pub struct SteamClient {}

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
    let steam = entity.world().resource::<bevy_steamworks::Client>().clone();

    world.entity_mut(session).insert(Session);

    let options = config.to_options();
    let result = match target {
        ConnectTarget::Addr(addr) => steam
            .networking_sockets()
            .connect_by_ip_address(addr, options),
        ConnectTarget::Peer {
            steam_id,
            virtual_port,
        } => steam.networking_sockets().connect_p2p(
            NetworkingIdentity::new_steam_id(steam_id),
            virtual_port,
            options,
        ),
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

    world
        .entity_mut(session)
        .insert((SteamIo { conn }, PacketMtu(config.send_buffer_size)));
}
