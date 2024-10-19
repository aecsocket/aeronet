//! Transmission of unreliable byte sequences (packets) along a [`Session`].

use {
    crate::{connection::Session, util::InitComponentExt},
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
    web_time::Instant,
};

#[derive(Debug)]
pub(crate) struct PacketPlugin;

impl Plugin for PacketPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PacketBuffersCapacity>()
            .register_type::<PacketBuffersCapacity>()
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
/// [`PacketBuffers`] has two [`PacketBuffer`]s which are thin wrappers over
/// ring buffers from [`ringbuf`]:
///
/// ```
/// use {aeronet_io::packet::PacketBuffers, bevy_ecs::prelude::*};
///
/// fn print_all_packets(mut sessions: Query<(Entity, &mut PacketBuffers)>) {
///     for (session, mut packet_bufs) in &mut sessions {
///         for packet in packet_bufs.recv.drain() {
///             println!("Received packet from {session}: {packet:?}");
///         }
///
///         println!("Sending out OK along {session}");
///         packet_bufs.send.push(b"OK"[..].into());
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
/// default, when creating this component, the capacity is given by
/// [`PacketBuffersCapacity::compute_from`].
#[derive(Component)]
pub struct PacketBuffers {
    /// Buffer of packets received along the IO layer during [`IoSet::Poll`].
    ///
    /// Each packet in this buffer may be of arbitrary size - it may be 0 bytes
    /// or larger than the [`PacketMtu`] on this session.
    ///
    /// The [`Instant`] represents the first instant at which this packet was
    /// identified and effectively received.
    ///
    /// [`IoSet::Poll`]: crate::IoSet::Poll
    pub recv: PacketBuffer<(Instant, Bytes)>,
    /// Buffer of packets that will be drained and sent out along the IO layer
    /// during [`IoSet::Flush`].
    ///
    /// Each packet pushed into this buffer must have a length smaller than or
    /// equal to [`PacketMtu`].
    ///
    /// [`IoSet::Flush`]: crate::IoSet::Flush
    pub send: PacketBuffer<Bytes>,
}

impl PacketBuffers {
    /// Creates a new set of buffers with the same capacity for both receive
    /// and send buffers.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            recv: PacketBuffer::new(capacity),
            send: PacketBuffer::new(capacity),
        }
    }
}

/// Buffer of packets, either incoming or outgoing, on a [`PacketBuffers`].
///
/// We expose a wrapper type as the API instead of the [`HeapRb`] directly, as
/// [`ringbuf`] buffers require importing traits to use its types. To avoid
/// API consumers having to also import those traits, we expose this wrapper
/// type.
///
/// We also use a wrapper type instead of functions on the [`PacketBuffers`]
/// directly, as a function there would have to take a mutable reference to the
/// entire [`PacketBuffers`] value, which is overly restrictive when e.g.
/// reading from one and pushing into the other.
#[derive(Deref, DerefMut)]
pub struct PacketBuffer<T>(pub HeapRb<T>);

impl<T> PacketBuffer<T> {
    /// Creates a new buffer with capacity for the given number of packets.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self(HeapRb::new(capacity))
    }

    /// Pushes a packet in, potentially overwriting the last packet if the
    /// buffer is full.
    ///
    /// This is equivalent to [`push_overwrite`].
    ///
    /// - `recv.push` should only be called by the IO layer.
    /// - `send.push` should only be called by code above the IO layer.
    ///
    /// [`push_overwrite`]: ringbuf::traits::RingBuffer::push_overwrite
    pub fn push(&mut self, packet: T) {
        self.push_overwrite(packet);
    }

    /// Returns an iterator that removes packets one by one from this buffer.
    ///
    /// This is equivalent to [`pop_iter`].
    ///
    /// - `recv.drain` should only be called by code above the IO layer.
    /// - `send.drain` should only be called by the IO layer.
    ///
    /// [`pop_iter`]: ringbuf::traits::Consumer::pop_iter
    pub fn drain(&mut self) -> impl Iterator<Item = T> + '_ {
        self.pop_iter()
    }
}

/// Capacity provided to [`PacketBuffers::with_capacity`] when creating the
/// component on a [`Session`].
///
/// If this component is present on an entity before [`PacketBuffers`] is
/// created, this component's value will be used as the capacity. Otherwise,
/// this type is looked up as a resource, and that value is used as the
/// capacity.
///
/// By default, this is effectively an arbitrary value, since we have no hints
/// on how many packets may be sent or received per [`Update`]. If you have an
/// estimate for this value, you should update this resource.
///
/// If in doubt, it's better to overestimate the capacity and use a bit of extra
/// memory, rather than drop packets.
///
/// This value may also be used by implementations which need a buffer capacity
/// internally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deref, DerefMut, Component, Resource, Reflect)]
#[reflect(Component, Resource)]
pub struct PacketBuffersCapacity(pub usize);

impl Default for PacketBuffersCapacity {
    fn default() -> Self {
        Self(64)
    }
}

impl PacketBuffersCapacity {
    /// Computes the value passed to [`PacketBuffers::with_capacity`] when
    /// creating buffers for a given session.
    #[must_use]
    pub fn compute_from(world: &World, session: Entity) -> usize {
        world
            .get::<Self>(session)
            .unwrap_or_else(|| world.resource::<Self>())
            .0
    }
}

/// Maximum transmissible unit (packet length) of outgoing packets on a
/// [`Session`].
///
/// Sent packets must have a length smaller than or equal to this value. Note
/// that this value may be arbitrarily large, and may even be [`usize::MAX`],
/// so you should not attempt to preallocate buffers of size [`PacketMtu`].
///
/// This component must only be mutated by the IO layer.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Deref, DerefMut, Component, Reflect,
)]
#[reflect(Component)]
pub struct PacketMtu(pub usize);

/// Safe estimate for the [maximum transmissible unit](PacketMtu) of a packet
/// sent using IP.
///
/// This value comes from <https://blog.cloudflare.com/increasing-ipv6-mtu/>.
///
/// If an IO layer sends packets over the internet, it should use this value
/// minus any overhead as its [`PacketMtu`].
pub const IP_MTU: usize = 1400;

/// Round-trip time of packets on a [`Session`] as computed by the IO layer.
///
/// At the IO layer, the exact definition of round-trip time is left vague,
/// since this is highly implementation-specific.
///
/// This component may not be present on sessions whose IO layers don't provide
/// an RTT estimate.
///
/// This component must only be mutated by the IO layer.
#[derive(Debug, Clone, Copy, Deref, DerefMut, Component, Reflect)]
#[reflect(Component)]
#[doc(alias = "ping")]
#[doc(alias = "latency")]
pub struct PacketRtt(pub Duration);

/// Statistics for the IO layer of a [`Session`].
///
/// As a component, these represent the total values since this session was
/// spawned.
///
/// This component is added alongside [`PacketBuffers`] automatically, but the
/// IO layer must mutate these values by itself.
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
        .add(|entity: Entity, world: &mut World| {
            if world.entity(entity).contains::<PacketBuffers>() {
                return;
            }

            let capacity = PacketBuffersCapacity::compute_from(world, entity);
            world
                .entity_mut(entity)
                .insert(PacketBuffers::with_capacity(capacity));
        })
        .init_component::<PacketMtu>()
        .init_component::<PacketStats>();
}
