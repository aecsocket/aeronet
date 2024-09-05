use std::{num::Saturating, usize};

use aeronet::{
    io::{PacketBuffers, PacketMtu},
    session::DROP_DISCONNECT_REASON,
    stats::SessionStats,
};
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bytes::Bytes;
use sync_wrapper::SyncWrapper;
use tracing::{trace, trace_span};

#[derive(Debug)]
pub struct ChannelSessionPlugin;

impl Plugin for ChannelSessionPlugin {
    fn build(&self, app: &mut App) {}
}

pub trait SpawnChannelSessionsExt {
    fn spawn_channel_sessions(&mut self) -> (Entity, Entity);
}

impl SpawnChannelSessionsExt for Commands<'_, '_> {
    fn spawn_channel_sessions(&mut self) -> (Entity, Entity) {
        let (send_packet_a, recv_packet_a) = flume::unbounded::<Bytes>();
        let (send_packet_b, recv_packet_b) = flume::unbounded::<Bytes>();
        let (send_dc_a, recv_dc_a) = oneshot::channel::<String>();
        let (send_dc_b, recv_dc_b) = oneshot::channel::<String>();

        let shared = (
            PacketBuffers::default(),
            PacketMtu(usize::MAX),
            SessionStats::default(),
        );
        let a = self
            .spawn((
                ChannelIo {
                    send_packet: send_packet_a,
                    recv_packet: recv_packet_b,
                    send_dc: Some(SyncWrapper::new(send_dc_a)),
                    recv_dc: SyncWrapper::new(recv_dc_b),
                },
                shared.clone(),
            ))
            .id();
        let b = self
            .spawn((
                ChannelIo {
                    send_packet: send_packet_b,
                    recv_packet: recv_packet_a,
                    send_dc: Some(SyncWrapper::new(send_dc_b)),
                    recv_dc: SyncWrapper::new(recv_dc_a),
                },
                shared,
            ))
            .id();
        (a, b)
    }
}

#[derive(Debug, Component)]
struct ChannelIo {
    send_packet: flume::Sender<Bytes>,
    recv_packet: flume::Receiver<Bytes>,
    send_dc: Option<SyncWrapper<oneshot::Sender<String>>>,
    recv_dc: SyncWrapper<oneshot::Receiver<String>>,
}

impl Drop for ChannelIo {
    fn drop(&mut self) {
        if let Some(sender) = self.send_dc.take() {
            let _ = sender.into_inner().send(DROP_DISCONNECT_REASON.to_owned());
        }
    }
}

fn recv(
    mut query: Query<(
        Entity,
        &mut ChannelIo,
        &mut PacketBuffers,
        &mut SessionStats,
    )>,
) {
    for (session, mut io, mut bufs, mut stats) in &mut query {
        let span = trace_span!("session", ?session);
        let _span = span.enter();

        let mut num_packets = Saturating(0);
        let mut num_bytes = Saturating(0);
        for packet in io.recv_packet.try_iter() {
            num_packets += 1;
            stats.packets_recv += 1;

            num_bytes += packet.len();
            stats.bytes_recv += packet.len();

            bufs.recv.push(packet);
        }

        trace!(
            num_packets = num_packets.0,
            num_bytes = num_bytes.0,
            "Received packets"
        );
    }
}
