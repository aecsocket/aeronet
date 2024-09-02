use std::num::Wrapping;

use aeronet::server::RemoteClient;
use bevy_ecs::prelude::*;
use bevy_reflect::prelude::*;
use bytes::Bytes;
use thiserror::Error;

use crate::{client::LocalChannelClient, server::RemoteChannelClient};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("disconnected")]
pub struct Disconnected;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub struct MessageKey(Wrapping<u16>);

impl MessageKey {
    #[inline]
    pub const fn from_raw(seq: u16) -> Self {
        Self(Wrapping(seq))
    }

    #[inline]
    pub const fn into_raw(self) -> u16 {
        self.0 .0
    }

    #[inline]
    pub fn get_and_increment(&mut self) -> Self {
        let seq = *self;
        self.0 += 1;
        seq
    }
}

pub trait SpawnChannelClientExt {
    fn spawn_channel_client(&mut self, server: Entity) -> (Entity, Entity);
}

impl SpawnChannelClientExt for Commands<'_, '_> {
    fn spawn_channel_client(&mut self, server: Entity) -> (Entity, Entity) {
        const ACK_BUF_CAP: usize = 16;

        let (send_c2s, recv_c2s) = flume::unbounded::<Bytes>();
        let (send_s2c, recv_s2c) = flume::unbounded::<Bytes>();
        let (send_c2s_acks, recv_c2s_acks) = flume::bounded::<()>(ACK_BUF_CAP);
        let (send_s2c_acks, recv_s2c_acks) = flume::bounded::<()>(ACK_BUF_CAP);
        let (send_c2s_dc, recv_c2s_dc) = flume::bounded::<String>(1);
        let (send_s2c_dc, recv_s2c_dc) = flume::bounded::<String>(1);

        let local = self
            .spawn(LocalChannelClient::new(
                send_c2s,
                recv_s2c,
                send_c2s_acks,
                recv_s2c_acks,
                send_c2s_dc,
                recv_s2c_dc,
            ))
            .id();

        let remote = self
            .spawn((
                RemoteClient::new(server),
                RemoteChannelClient::new(
                    recv_c2s,
                    send_s2c,
                    recv_c2s_acks,
                    send_s2c_acks,
                    recv_c2s_dc,
                    send_s2c_dc,
                ),
            ))
            .id();

        (local, remote)
    }
}
