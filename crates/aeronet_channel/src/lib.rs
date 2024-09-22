#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

use {
    aeronet_io::{
        connection::{
            Connected, Disconnect, DisconnectReason, Disconnected, Session, DROP_DISCONNECT_REASON,
        },
        packet::{PacketBuffers, PacketBuffersCapacity, PacketMtu, PacketStats},
        AeronetIoPlugin, IoSet,
    },
    bevy_app::prelude::*,
    bevy_ecs::{prelude::*, world::Command},
    bytes::Bytes,
    std::{cmp, num::Saturating},
    sync_wrapper::SyncWrapper,
    thiserror::Error,
    tracing::{trace, trace_span},
};

/// Allows using [`ChannelIo`].
#[derive(Debug)]
pub struct ChannelIoPlugin;

impl Plugin for ChannelIoPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<AeronetIoPlugin>() {
            app.add_plugins(AeronetIoPlugin);
        }

        app.add_systems(PreUpdate, poll.in_set(IoSet::Poll))
            .add_systems(PostUpdate, flush.in_set(IoSet::Flush))
            .observe(on_io_added)
            .observe(on_disconnect);
    }
}

/// [`aeronet_io`] layer using in-memory MPSC channels.
///
/// Use [`ChannelIo::open`] to open a connection between two entities.
#[derive(Debug, Component)]
pub struct ChannelIo {
    send_packet: flume::Sender<Bytes>,
    recv_packet: flume::Receiver<Bytes>,
    send_dc: Option<SyncWrapper<oneshot::Sender<String>>>,
    recv_dc: SyncWrapper<oneshot::Receiver<String>>,
}

impl ChannelIo {
    /// Creates a [`Command`] to open a [`ChannelIo`] pair between two entities.
    ///
    /// When the command is applied, entities `a` and `b` must exist in the
    /// world, otherwise the command will panic. If your entities are in
    /// separate worlds, use [`ChannelIo::with_capacity`] to manually create
    /// a [`ChannelIo`] pair, and add the components to the target entities
    /// manually.
    ///
    /// The buffer capacity used when creating the IO pair is the maximum of
    /// the capacities computed by [`PacketBuffersCapacity::compute_from`] for
    /// each entity.
    ///
    /// # Examples
    ///
    /// ```
    /// use {aeronet_channel::ChannelIo, bevy_ecs::{prelude::*, world::Command}};
    ///
    /// # fn run(mut commands: Commands, world: &mut World) {
    /// let a = commands.spawn_empty().id();
    /// let b = commands.spawn_empty().id();
    ///
    /// // using `Commands`
    /// commands.add(ChannelIo::open(a, b));
    ///
    /// // using mutable `World` access
    /// ChannelIo::open(a, b).apply(world);
    /// # }
    /// ```
    #[must_use]
    pub fn open(a: Entity, b: Entity) -> impl Command {
        move |world: &mut World| {
            let capacity = cmp::max(
                PacketBuffersCapacity::compute_from(world, a),
                PacketBuffersCapacity::compute_from(world, b),
            );
            let (io_a, io_b) = Self::with_capacity(capacity);

            world.entity_mut(a).insert(io_a);
            world.entity_mut(b).insert(io_b);
        }
    }

    /// Creates a [`ChannelIo`] pair with a given packet buffer capacity.
    ///
    /// See [`PacketBuffers`] for how to choose a capacity value.
    ///
    /// If the target entities already exist in the same world, prefer using
    /// [`ChannelIo::open`] and applying the resulting command. However, if your
    /// entities exist in separate worlds (e.g. a client and a server world, as
    /// part of a sub-app), you may want to create the IO pair and set up your
    /// entities manually.
    ///
    /// # Examples
    ///
    /// ```
    /// use {aeronet_channel::ChannelIo, bevy_ecs::prelude::*};
    ///
    /// # fn run(client_world: &mut World, server_world: &mut World) {
    /// let (client_io, server_io) = ChannelIo::with_capacity(64);
    /// client_world.spawn(client_io);
    /// server_world.spawn(server_io);
    /// # }
    /// ```
    #[must_use]
    pub fn with_capacity(capacity: usize) -> (Self, Self) {
        let (send_packet_a, recv_packet_a) = flume::bounded(capacity);
        let (send_packet_b, recv_packet_b) = flume::bounded(capacity);
        let (send_dc_a, recv_dc_a) = oneshot::channel();
        let (send_dc_b, recv_dc_b) = oneshot::channel();

        (
            Self {
                send_packet: send_packet_a,
                recv_packet: recv_packet_b,
                send_dc: Some(SyncWrapper::new(send_dc_a)),
                recv_dc: SyncWrapper::new(recv_dc_b),
            },
            Self {
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

/// [`ChannelIo`] error when the peer drops its channel.
#[derive(Debug, Clone, Error)]
#[error("channel disconnected")]
pub struct ChannelDisconnected;

fn on_io_added(trigger: Trigger<OnAdd, ChannelIo>, mut commands: Commands) {
    let session = trigger.entity();
    commands
        .entity(session)
        .insert((Session, Connected, PacketMtu(usize::MAX)));
}

fn on_disconnect(trigger: Trigger<Disconnect>, mut sessions: Query<&mut ChannelIo>) {
    let session = trigger.entity();
    let Disconnect { reason } = trigger.event();
    let Ok(mut io) = sessions.get_mut(session) else {
        return;
    };

    if let Some(send_dc) = io.send_dc.take() {
        let _ = send_dc.into_inner().send(reason.clone());
    }
}

fn poll(
    mut commands: Commands,
    mut sessions: Query<(Entity, &mut ChannelIo, &mut PacketBuffers, &mut PacketStats)>,
) {
    for (session, mut io, mut bufs, mut stats) in &mut sessions {
        let span = trace_span!("poll", %session);
        let _span = span.enter();

        let dc_reason = match io.recv_dc.get_mut().try_recv() {
            Ok(reason) => Some(DisconnectReason::Peer(reason)),
            Err(oneshot::TryRecvError::Disconnected) => {
                Some(DisconnectReason::Error(ChannelDisconnected.into()))
            }
            Err(oneshot::TryRecvError::Empty) => None,
        };
        if let Some(reason) = dc_reason {
            commands.trigger_targets(Disconnected { reason }, session);
            continue;
        }

        let mut num_packets = Saturating(0);
        let mut num_bytes = Saturating(0);
        for packet in io.recv_packet.try_iter() {
            num_packets += 1;
            stats.packets_recv += 1;

            num_bytes += packet.len();
            stats.bytes_recv += packet.len();

            bufs.push_recv(packet);
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
        let span = trace_span!("flush", %session);
        let _span = span.enter();

        let mut num_packets = Saturating(0);
        let mut num_bytes = Saturating(0);
        for packet in bufs.drain_send() {
            num_packets += 1;
            stats.packets_sent += 1;

            num_bytes += packet.len();
            stats.bytes_sent += packet.len();

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
