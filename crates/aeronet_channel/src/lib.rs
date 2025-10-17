#![cfg_attr(docsrs_aeronet, feature(doc_cfg))]
#![doc = include_str!("../README.md")]

use {
    aeronet_io::{
        AeronetIoPlugin, IoSystems, Session, SessionEndpoint,
        connection::{DROP_DISCONNECT_REASON, Disconnect, DisconnectReason, Disconnected},
        packet::RecvPacket,
    },
    bevy_app::prelude::*,
    bevy_ecs::prelude::*,
    bevy_platform::time::Instant,
    bytes::Bytes,
    core::num::Saturating,
    derive_more::{Display, Error},
    sync_wrapper::SyncWrapper,
    tracing::{trace, trace_span},
};

/// Allows using [`ChannelIo`].
pub struct ChannelIoPlugin;

impl Plugin for ChannelIoPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<AeronetIoPlugin>() {
            app.add_plugins(AeronetIoPlugin);
        }

        app.add_systems(PreUpdate, poll.in_set(IoSystems::Poll))
            .add_systems(PostUpdate, flush.in_set(IoSystems::Flush))
            .add_observer(on_io_added)
            .add_observer(on_disconnect);
    }
}

/// [`aeronet_io`] layer using in-memory MPSC channels.
///
/// Use [`ChannelIo::open`] to open a connection between two entities.
#[derive(Debug, Component)]
pub struct ChannelIo {
    tx_packet: flume::Sender<Bytes>,
    rx_packet: flume::Receiver<Bytes>,
    tx_dc: Option<SyncWrapper<oneshot::Sender<String>>>,
    rx_dc: SyncWrapper<oneshot::Receiver<String>>,
}

impl ChannelIo {
    /// Creates a [`ChannelIo`] pair.
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
    /// let (client_io, server_io) = ChannelIo::new();
    /// client_world.spawn(client_io);
    /// server_world.spawn(server_io);
    /// # }
    /// ```
    #[must_use]
    pub fn new() -> (Self, Self) {
        let (tx_packet_a, rx_packet_a) = flume::unbounded();
        let (tx_packet_b, rx_packet_b) = flume::unbounded();
        let (tx_dc_a, rx_dc_a) = oneshot::channel();
        let (tx_dc_b, rx_dc_b) = oneshot::channel();

        (
            Self {
                tx_packet: tx_packet_a,
                rx_packet: rx_packet_b,
                tx_dc: Some(SyncWrapper::new(tx_dc_a)),
                rx_dc: SyncWrapper::new(rx_dc_b),
            },
            Self {
                tx_packet: tx_packet_b,
                rx_packet: rx_packet_a,
                tx_dc: Some(SyncWrapper::new(tx_dc_b)),
                rx_dc: SyncWrapper::new(rx_dc_a),
            },
        )
    }

    /// Creates a [`Command`] to open a [`ChannelIo`] pair between two entities.
    ///
    /// When the command is applied, entities `a` and `b` must exist in the
    /// world, otherwise the command will panic. If your entities are in
    /// separate worlds, use [`ChannelIo::new`] to manually create a
    /// [`ChannelIo`] pair, and add the components to the target entities
    /// manually.
    ///
    /// # Examples
    ///
    /// ```
    /// use {aeronet_channel::ChannelIo, bevy_ecs::prelude::*};
    ///
    /// # fn run(mut commands: Commands, world: &mut World) {
    /// let a = commands.spawn_empty().id();
    /// let b = commands.spawn_empty().id();
    ///
    /// // using `Commands`
    /// commands.queue(ChannelIo::open(a, b));
    ///
    /// // using mutable `World` access
    /// ChannelIo::open(a, b).apply(world);
    /// # }
    /// ```
    #[must_use]
    pub fn open(a: Entity, b: Entity) -> impl Command {
        move |world: &mut World| {
            let (io_a, io_b) = Self::new();
            world.entity_mut(a).insert(io_a);
            world.entity_mut(b).insert(io_b);
        }
    }
}

impl Drop for ChannelIo {
    fn drop(&mut self) {
        if let Some(tx_dc) = self.tx_dc.take() {
            _ = tx_dc.into_inner().send(DROP_DISCONNECT_REASON.to_owned());
        }
    }
}

/// [`ChannelIo`] error when the peer drops its channel.
#[derive(Debug, Clone, Display, Error)]
#[display("channel disconnected")]
pub struct ChannelDisconnected;

const MTU: usize = usize::MAX;

fn on_io_added(trigger: On<Add, ChannelIo>, mut commands: Commands) {
    let entity = trigger.event_target();
    let session = Session::new(Instant::now(), MTU);
    commands.entity(entity).insert((SessionEndpoint, session));
}

fn on_disconnect(trigger: On<Disconnect>, mut sessions: Query<&mut ChannelIo>) {
    let entity = trigger.event_target();
    let Ok(mut io) = sessions.get_mut(entity) else {
        return;
    };

    if let Some(tx_dc) = io.tx_dc.take() {
        _ = tx_dc.into_inner().send(trigger.reason.clone());
    }
}

fn poll(mut commands: Commands, mut sessions: Query<(Entity, &mut Session, &mut ChannelIo)>) {
    for (entity, mut session, mut io) in &mut sessions {
        let span = trace_span!("poll", %entity);
        let _span = span.enter();

        let dc_reason = match io.rx_dc.get_mut().try_recv() {
            Ok(reason) => Some(DisconnectReason::by_peer(reason)),
            Err(oneshot::TryRecvError::Disconnected) => {
                Some(DisconnectReason::by_error(ChannelDisconnected))
            }
            Err(oneshot::TryRecvError::Empty) => None,
        };
        if let Some(reason) = dc_reason {
            commands.trigger(Disconnected { entity, reason });
            continue;
        }

        let mut num_packets = Saturating(0);
        let mut num_bytes = Saturating(0);
        for packet in io.rx_packet.try_iter() {
            num_packets += 1;
            session.stats.packets_recv += 1;

            num_bytes += packet.len();
            session.stats.bytes_recv += packet.len();

            session.recv.push(RecvPacket {
                recv_at: Instant::now(),
                payload: packet,
            });
        }

        trace!(
            num_packets = num_packets.0,
            num_bytes = num_bytes.0,
            "Received packets"
        );
    }
}

fn flush(mut sessions: Query<(Entity, &mut Session, &ChannelIo)>) {
    for (entity, mut session, io) in &mut sessions {
        let span = trace_span!("flush", %entity);
        let _span = span.enter();

        // explicit deref so we can access disjoint fields
        let session = &mut *session;
        let mut num_packets = Saturating(0);
        let mut num_bytes = Saturating(0);
        for packet in session.send.drain(..) {
            num_packets += 1;
            session.stats.packets_sent += 1;

            num_bytes += packet.len();
            session.stats.bytes_sent += packet.len();

            // handle connection errors in `poll`
            _ = io.tx_packet.try_send(packet);
        }

        trace!(
            num_packets = num_packets.0,
            num_bytes = num_bytes.0,
            "Flushed packets"
        );
    }
}
