use {
    crate::{util::InitComponentExt, Session},
    bevy_app::prelude::*,
    bevy_derive::{Deref, DerefMut},
    bevy_ecs::prelude::*,
    bevy_reflect::prelude::*,
    bytes::Bytes,
    derive_more::{Add, AddAssign, Sub, SubAssign},
    ringbuf::{
        traits::{Consumer, RingBuffer},
        HeapRb,
    },
    std::{num::Saturating, time::Duration},
};

#[derive(Debug)]
pub(crate) struct PacketPlugin;

impl Plugin for PacketPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DefaultPacketBuffersCapacity>()
            .register_type::<DefaultPacketBuffersCapacity>()
            .register_type::<PacketMtu>()
            .register_type::<PacketRtt>()
            .register_type::<PacketStats>()
            .observe(on_connecting);
    }
}

/// Buffers for incoming and outgoing packets on a [`Session`].
///
/// A packet is an arbitrary sequence of bytes which may be of any length,
/// however the IO layer may refuse to send a packet if it is too long.
/// This layer does not provide any guarantees on packet delivery. Packets may
/// be delayed, lost, or even duplicated. However, packets are guaranteed to not
/// be corrupted, truncated, or extended in transit. If this does happen, the IO
/// layer must treat it as a lost packet, and drop it.
///
/// # Sending and receiving
///
/// [`PacketBuffers`] has two [`ringbuf`] packet ring buffers. Since these
/// require importing traits to use, convenience functions are provided:
/// - use [`PacketBuffers::drain_recv`] to drain the received packets,
///   equivalent to [`pop_iter`] on [`PacketBuffers::recv`]
/// - use [`PacketBuffers::push_send`] to enqueue a packet for sending,
///   equivalent to [`push_overwrite`] on [`PacketBuffers::send`]
///
/// ```
/// use {aeronet_io::PacketBuffers, bevy::prelude::*};
///
/// fn print_all_packets(mut sessions: Query<(Entity, &mut PacketBuffers)>) {
///     for (session, mut packet_bufs) in &mut sessions {
///         for packet in packet_bufs.drain_recv() {
///             info!("Received packet from {session}: {packet:?}");
///         }
///
///         info!("Sending out OK along {session}");
///         packet_bufs.push_send(b"OK"[..].into());
///     }
/// }
/// ```
///
/// Sent packets must have a length smaller than or equal to [`PacketMtu`],
/// otherwise the packet will be discarded.
///
/// # Buffers
///
/// Internally, the buffers are implemented as ring buffers from the [`ringbuf`]
/// crate. This is used instead of a [`Vec`] or other dynamically resizable
/// collection type to avoid unbounded growth, and to avoid allocations in
/// hot-path IO layer code. However, this means that if you do not consume
/// packets from [`PacketBuffers::recv`] often enough, or buffer too many
/// packets into [`PacketBuffers::send`], then you will lose some packets.
///
/// You can think of the capacity of each buffer in this struct as an upper
/// bound on how many packets we can send and receive per [`Update`]. By
/// default, when creating this component using [`FromWorld`], the capacity is
/// given by the value in [`DefaultPacketBuffersCapacity`].
#[derive(Component)]
pub struct PacketBuffers {
    /// Buffer of packets received along the IO layer during [`IoSet::Poll`].
    ///
    /// Each packet in this buffer may be of arbitrary size - it may be 0 bytes
    /// or larger than the [`PacketMtu`] on this session.
    ///
    /// [`IoSet::Poll`]: crate::IoSet::Poll
    pub recv: HeapRb<Bytes>,
    /// Buffer of packets that will be drained and sent out along the IO layer
    /// during [`IoSet::Flush`].
    ///
    /// Each packet pushed into this buffer must have a length smaller than or
    /// equal to [`PacketMtu`].
    ///
    /// [`IoSet::Flush`]: crate::IoSet::Flush
    pub send: HeapRb<Bytes>,
    capacity: usize,
}

impl PacketBuffers {
    /// Creates a new set of buffers with the same capacity for both receive
    /// and send buffers.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            recv: HeapRb::new(capacity),
            send: HeapRb::new(capacity),
            capacity,
        }
    }

    /// Gets the capacity which these buffers were initialized with.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Pushes a packet into [`PacketBuffers::recv`], potentially overwriting
    /// the last packet if the buffer is full.
    ///
    /// This should only be called by the IO layer implementation.
    pub fn push_recv(&mut self, packet: Bytes) {
        self.recv.push_overwrite(packet);
    }

    /// Returns an iterator that removes packets one by one from
    /// [`PacketBuffers::recv`].
    ///
    /// This should only be called by code above the IO layer.
    pub fn drain_recv(&mut self) -> impl Iterator<Item = Bytes> + '_ {
        self.recv.pop_iter()
    }

    /// Pushes a packet into [`PacketBuffers::send`], potentially overwriting
    /// the last packet if the buffer is full.
    ///
    /// This should only be called by code above the IO layer.
    pub fn push_send(&mut self, packet: Bytes) {
        self.send.push_overwrite(packet);
    }

    /// Returns an iterator that removes packets one by one from
    /// [`PacketBuffers::recv`].
    ///
    /// This should only be called by the IO layer implementation.
    pub fn drain_send(&mut self) -> impl Iterator<Item = Bytes> + '_ {
        self.send.pop_iter()
    }
}

/// Default capacity of the buffers when creating a [`PacketBuffers`] via its
/// [`FromWorld`] implementation.
///
/// By default, this is effectively an arbitrary value, since we have no hints
/// on how many packets may be sent or received per [`Update`]. If you have an
/// estimate for this value, you should update this resource.
///
/// If in doubt, it's better to overestimate the capacity and use a bit of extra
/// memory, rather than drop packets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deref, DerefMut, Resource, Reflect)]
#[reflect(Resource)]
pub struct DefaultPacketBuffersCapacity(pub usize);

impl Default for DefaultPacketBuffersCapacity {
    fn default() -> Self {
        Self(64)
    }
}

impl FromWorld for PacketBuffers {
    fn from_world(world: &mut World) -> Self {
        let capacity = **world.resource::<DefaultPacketBuffersCapacity>();
        Self::with_capacity(capacity)
    }
}

/// Maximum transmissible unit (packet length) of outgoing packets on a
/// [`Session`].
///
/// Sent packets must have a length smaller than or equal to this value. Note
/// that this value may be arbitrarily large, and may even be [`usize::MAX`],
/// so you should not attempt to preallocate buffers of size [`PacketMtu`].
///
/// This component must only be mutated by the IO layer implementation.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Deref, DerefMut, Component, Reflect,
)]
#[reflect(Component)]
pub struct PacketMtu(pub usize);

/// Round-trip time of packets on a [`Session`] as computed by the IO layer.
///
/// At the IO layer, the exact definition of round-trip time is left vague,
/// since this is highly implementation-specific.
///
/// This component may not be present on sessions whose IO layers don't provide
/// an RTT estimate.
///
/// This component must only be mutated by the IO layer.
#[derive(Debug, Clone, Deref, DerefMut, Component, Reflect)]
#[reflect(Component)]
#[doc(alias = "ping")]
#[doc(alias = "latency")]
pub struct PacketRtt(pub Duration);

/// Statistics for the IO layer of a [`Session`].
///
/// As a component, these represent the total values since this session was
/// spawned.
#[derive(Debug, Clone, Copy, Default, Component, Reflect, Add, AddAssign, Sub, SubAssign)]
#[reflect(Component)]
pub struct PacketStats {
    /// Number of packets received into [`PacketBuffers::recv`].
    pub packets_recv: Saturating<usize>,
    /// Number of packets sent out from [`PacketBuffers::send`].
    pub packets_sent: Saturating<usize>,
    /// Sum of the byte lengths of packets received into
    /// [`PacketBuffers::recv`].
    pub bytes_recv: Saturating<usize>,
    /// Sum of the byte lengths of packets sent out from
    /// [`PacketBuffers::send`].
    pub bytes_sent: Saturating<usize>,
}

fn on_connecting(trigger: Trigger<OnAdd, Session>, mut commands: Commands) {
    let session = trigger.entity();
    commands
        .entity(session)
        .init_component::<PacketBuffers>()
        .init_component::<PacketMtu>()
        .init_component::<PacketStats>();
}
