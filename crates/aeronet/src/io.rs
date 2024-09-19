//! Layer for handling data transfer over a [session].
//!
//! The IO layer is responsible for reading packets from, and sending packets
//! to, some underlying primitive for data transfer (e.g. a UDP socket, MPSC
//! channel, or WebTransport connection). The process of sending and receiving
//! packets is abstracted away under items from this crate, so that higher-level
//! logic such as transports do not have to worry about what IO layer is being
//! used to transmit their packets.
//!
//! # Packets
//!
//! A packet is an arbitrary sequence of bytes which may be of any length,
//! however the IO layer may refuse to send a packet if it is too long.
//! This layer does not provide any guarantees on packet delivery. Packets may
//! be delayed, lost, or even duplicated. However, packets are guaranteed to not
//! be corrupted, truncated, or extended in transit. If this does happen, the IO
//! layer must treat it as a lost packet, and drop it.
//!
//! This layer is only really useful to you if you are implementing your own IO
//! layer, or you are implementing your own transport layer. For most purposes,
//! you should be using a higher-level API than just packets, since you probably
//! need reliability, ordering and fragmentation.
//!
//! # Sending and receiving
//!
//! [`PacketBuffers`] has two [`ringbuf`] packet ring buffers. Since these
//! require importing traits to use, convenience functions are provided:
//! - use [`PacketBuffers::drain_recv`] to drain the received packets,
//!   equivalent to [`pop_iter`] on [`PacketBuffers::recv`]
//! - use [`PacketBuffers::push_send`] to enqueue a packet for sending,
//!   equivalent to [`push_overwrite`] on [`PacketBuffers::send`]
//!
//! ```
//! use bevy::prelude::*;
//! use aeronet::io::PacketBuffers;
//!
//! fn print_all_packets(
//!     mut sessions: Query<(Entity, &mut PacketBuffers)>,
//! ) {
//!     for (session, mut packet_bufs) in &mut sessions {
//!         for packet in packet_bufs.drain_recv() {
//!             info!("Received packet from {session:?}: {packet:?}");
//!         }
//!
//!         info!("Sending out OK along {session:?}");
//!         packet_bufs.push_send(b"OK"[..].into());
//!     }
//! }
//! ```
//!
//! Sent packets must have a length smaller than or equal to [`PacketMtu`],
//! otherwise the packet may be discarded, and a warning may be logged (this is
//! up to the implementation).
//!
//! [session]: crate::session
//! [`pop_iter`]: ringbuf::traits::Consumer::pop_iter
//! [`push_overwrite`]: ringbuf::traits::RingBuffer::push_overwrite

use std::{num::Saturating, time::Duration};

use bevy_app::prelude::*;
use bevy_derive::{Deref, DerefMut};
use bevy_ecs::prelude::*;
use bevy_reflect::prelude::*;
use bytes::Bytes;
use derive_more::{Add, AddAssign, Sub, SubAssign};
use ringbuf::{
    traits::{Consumer, RingBuffer},
    HeapRb,
};

#[derive(Debug)]
pub(crate) struct IoPlugin;

impl Plugin for IoPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(PreUpdate, IoSet::Poll)
            .configure_sets(PostUpdate, IoSet::Flush)
            .register_type::<PacketMtu>()
            .register_type::<IoStats>();
    }
}

/// Set for scheduling [IO layer] systems.
///
/// [IO layer]: crate::io
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum IoSet {
    /// Progressing the connection, handling disconnects, and receiving packets
    /// from the IO layer.
    Poll,
    /// Sending buffered packets out over the IO layer.
    Flush,
}

/// Buffers for incoming and outgoing packets on a [session].
///
/// See the [IO layer] on info for how sending and receiving packets is handled.
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
/// default, the capacity is [`PACKET_BUF_CAP`]. If in doubt, it's usually
/// better to overestimate the capacity and allocate some extra unused memory,
/// than to underestimate and drop packets.
///
/// [session]: crate::session
/// [IO layer]: crate::io
#[derive(Component)]
pub struct PacketBuffers {
    /// Buffer of packets received from the IO layer during [`IoSet::Recv`].
    ///
    /// Each packet in this buffer may be of arbitrary size - it may be 0 bytes
    /// or larger than the [`PacketMtu`] on this session.
    pub recv: HeapRb<Bytes>,
    /// Buffer of packets that will be drained and sent out to the IO layer
    /// during [`IoSet::Send`].
    ///
    /// Each packet pushed into this buffer must have a length smaller than or
    /// equal to [`PacketMtu`].
    pub send: HeapRb<Bytes>,
}

impl PacketBuffers {
    /// Creates a new set of buffers with the same capacity for both receive
    /// and send buffers.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            recv: HeapRb::new(capacity),
            send: HeapRb::new(capacity),
        }
    }

    /// Pushes a packet into [`PacketBuffers::recv`], potentially overwriting
    /// the last packet if the buffer is full.
    ///
    /// This should only be called by the IO layer code.
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
    /// This should only be called by the IO layer code.
    pub fn drain_send(&mut self) -> impl Iterator<Item = Bytes> + '_ {
        self.send.pop_iter()
    }
}

/// Default capacity for the size of the buffers in [`PacketBuffers`].
///
/// The value here is effectively arbitrary, since we have no hints on how many
/// packets we may be sending or receiving per [`Update`].
pub const PACKET_BUF_CAP: usize = 64;

impl Default for PacketBuffers {
    fn default() -> Self {
        Self::new(PACKET_BUF_CAP)
    }
}

/// Maximum transmissible unit (packet length) of outgoing packets on a
/// [session].
///
/// Sent packets must have a length smaller than or equal to this value. Note
/// that this value may be arbitrarily large, and may even be [`usize::MAX`],
/// so you should not attempt to preallocate buffers of size [`PacketMtu`].
///
/// This component must only be mutated by the IO layer.
///
/// [session]: crate::session
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deref, DerefMut, Component, Reflect, Default,
)]
#[reflect(Component)]
pub struct PacketMtu(pub usize);

/// Round-trip time of packets on a [session] as computed by the [IO layer].
///
/// See [RTT] for an explanation of round-trip time.
///
/// This component may not be present on sessions whose IO layers don't provide
/// an RTT estimate.
///
/// This component must only be mutated by the IO layer.
///
/// [session]: crate::session
/// [IO layer]: crate::io
/// [RTT]: crate::rtt
#[derive(Debug, Clone, Deref, DerefMut, Component, Reflect)]
#[reflect(Component)]
pub struct PacketRtt(pub Duration);

/// Statistics for the [IO layer] of a [session].
///
/// As a component, these represent the total values since this session was
/// spawned.
///
/// [IO layer]: crate::io
/// [session]: crate::session
#[derive(Debug, Clone, Copy, Default, Component, Reflect, Add, AddAssign, Sub, SubAssign)]
#[reflect(Component)]
pub struct IoStats {
    /// Number of packets received into [`PacketBuffers::recv`].
    pub packets_recv: Saturating<usize>,
    /// Number of packets sent out from [`PacketBuffers::send`].
    pub packets_sent: Saturating<usize>,
    /// Sum of the byte lengths of packets received into [`PacketBuffers::recv`].
    pub bytes_recv: Saturating<usize>,
    /// Sum of the byte lengths of packets sent out from [`PacketBuffers::send`].
    pub bytes_sent: Saturating<usize>,
}
