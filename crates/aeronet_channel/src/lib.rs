#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

use std::{num::Saturating, usize};

use aeronet::{
    io::{IoPlugin, IoSet, PacketBuffers, PacketMtu, PacketStats, PACKET_BUF_CAP},
    session::{
        Connected, Disconnect, DisconnectReason, Disconnected, Session, SessionPlugin,
        DROP_DISCONNECT_REASON,
    },
};
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bytes::Bytes;
use ringbuf::traits::{Consumer, RingBuffer};
use sync_wrapper::SyncWrapper;
use thiserror::Error;
use tracing::{debug, debug_span, trace, trace_span};

#[derive(Debug)]
pub struct ChannelIoPlugin;

impl Plugin for ChannelIoPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<SessionPlugin>() {
            app.add_plugins(SessionPlugin);
        }
        if !app.is_plugin_added::<IoPlugin>() {
            app.add_plugins(IoPlugin);
        }

        app.add_systems(PreUpdate, poll.in_set(IoSet::Poll))
            .add_systems(PostUpdate, flush.in_set(IoSet::Flush))
            .observe(connect)
            .observe(disconnect);
    }
}

#[derive(Debug, Clone, Error)]
#[error("channel disconnected")]
pub struct ChannelDisconnected;

#[derive(Debug, Component)]
pub struct ChannelIo {
    send_packet: flume::Sender<Bytes>,
    recv_packet: flume::Receiver<Bytes>,
    send_dc: Option<SyncWrapper<oneshot::Sender<String>>>,
    recv_dc: SyncWrapper<oneshot::Receiver<String>>,
}

impl ChannelIo {
    #[must_use]
    pub fn new() -> (Self, Self) {
        Self::with_capacity(PACKET_BUF_CAP)
    }

    #[must_use]
    pub fn with_capacity(capacity: usize) -> (Self, Self) {
        let (send_packet_a, recv_packet_a) = flume::bounded(capacity);
        let (send_packet_b, recv_packet_b) = flume::bounded(capacity);
        let (send_dc_a, recv_dc_a) = oneshot::channel();
        let (send_dc_b, recv_dc_b) = oneshot::channel();

        (
            ChannelIo {
                send_packet: send_packet_a,
                recv_packet: recv_packet_b,
                send_dc: Some(SyncWrapper::new(send_dc_a)),
                recv_dc: SyncWrapper::new(recv_dc_b),
            },
            ChannelIo {
                send_packet: send_packet_b,
                recv_packet: recv_packet_a,
                send_dc: Some(SyncWrapper::new(send_dc_b)),
                recv_dc: SyncWrapper::new(recv_dc_a),
            },
        )
    }
}

impl Drop for ChannelIo {
    fn drop(&mut self) {
        if let Some(send_dc) = self.send_dc.take() {
            let _ = send_dc.into_inner().send(DROP_DISCONNECT_REASON.to_owned());
        }
    }
}

fn connect(trigger: Trigger<OnAdd, ChannelIo>, mut commands: Commands) {
    let session = trigger.entity();

    let span = debug_span!("connect", ?session);
    let _span = span.enter();

    debug!("Connecting");

    commands.entity(session).insert((
        Session,
        Connected,
        PacketMtu(usize::MAX),
        PacketStats::default(),
    ));
}

fn disconnect(
    trigger: Trigger<OnAdd, Disconnect>,
    mut sessions: Query<(&mut ChannelIo, &Disconnect)>,
) {
    let session = trigger.entity();
    let Ok((mut io, Disconnect(reason))) = sessions.get_mut(session) else {
        return;
    };

    let span = debug_span!("disconnect", ?session);
    let _span = span.enter();

    debug!("Disconnecting: {reason}");

    if let Some(send_dc) = io.send_dc.take() {
        let _ = send_dc.into_inner().send(reason.clone());
        debug!("Sent disconnect reason");
    } else {
        debug!("Disconnect reason has already been sent, ignoring this one");
    }
}

fn poll(
    mut commands: Commands,
    mut sessions: Query<(Entity, &mut ChannelIo, &mut PacketBuffers, &mut PacketStats)>,
) {
    for (session, mut io, mut bufs, mut stats) in &mut sessions {
        let span = trace_span!("poll", ?session);
        let _span = span.enter();

        let dc_reason = match io.recv_dc.get_mut().try_recv() {
            Ok(reason) => Some(DisconnectReason::Peer(reason)),
            Err(oneshot::TryRecvError::Disconnected) => {
                Some(DisconnectReason::Error(ChannelDisconnected.into()))
            }
            Err(oneshot::TryRecvError::Empty) => None,
        };
        if let Some(dc_reason) = dc_reason {
            commands.entity(session).insert(Disconnected(dc_reason));
            continue;
        }

        let mut num_packets = Saturating(0);
        let mut num_bytes = Saturating(0);
        for packet in io.recv_packet.try_iter() {
            num_packets += 1;
            stats.packets_in += 1;

            num_bytes += packet.len();
            stats.bytes_in += packet.len();

            bufs.recv.push_overwrite(packet);
        }

        trace!(
            num_packets = num_packets.0,
            num_bytes = num_bytes.0,
            "Received packets"
        );
    }
}

fn flush(mut sessions: Query<(Entity, &ChannelIo, &mut PacketBuffers, &mut PacketStats)>) {
    for (session, io, mut bufs, mut stats) in &mut sessions {
        let span = trace_span!("flush", ?session);
        let _span = span.enter();

        let mut num_packets = Saturating(0);
        let mut num_bytes = Saturating(0);
        for packet in bufs.send.pop_iter() {
            num_packets += 1;
            stats.packets_out += 1;

            num_bytes += packet.len();
            stats.bytes_out += packet.len();

            // handle connection errors in `poll`
            let _ = io.send_packet.try_send(packet);
        }

        trace!(
            num_packets = num_packets.0,
            num_bytes = num_bytes.0,
            "Flushed packets"
        );
    }
}
