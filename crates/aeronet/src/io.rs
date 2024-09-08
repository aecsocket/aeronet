//! Layer for handling data transfer over a [session].
//!
//! The IO layer is responsible for reading packets from, and sending packets
//! to, some underlying primitive for data transfer (e.g. a UDP socket, MPSC
//! channel, or WebTransport connection). The process of sending and receiving
//! packets is abstracted away under items from this crate, so that higher-level
//! logic such as transports do not have to worry about what IO layer is being
//! used to transmit their packets.
//!
//! The main types used at the IO layer are:
//! - [`PacketBuffers`] for sending and receiving packets
//! - [`PacketMtu`] for checking how large one of your sent packets may be
//!
//! # Packets
//!
//! A packet is an arbitrary sequence of bytes which may be of any length,
//! however the IO layer may refuse to send a packet if it is too long.
//! This layer does not provide any guarantees on packet delivery. Packets may
//! be delayed, lost, or even duplicated. However, packets are guaranteed to not
//! be corrupted, truncated, or extended in transit. How this is implemented is
//! up to the IO layer implementation, and it is perfectly valid to drop these
//! kinds of corrupted packets.
//!
//! This layer is only really useful to you if you are implementing your own IO
//! layer, or you are implementing your own transport layer. For most purposes,
//! you should be using a higher-level API than just packets, since you probably
//! need reliability, ordering and fragmentation. For this, see the
//! [transport layer].
//!
//! # Sending and receiving
//!
//! Use [`PacketBuffers::recv`] to read received packets, and
//! [`PacketBuffers::send`] to enqueue packets for sending.
//!
//! ```
//! use bevy::prelude::*;
//! use aeronet::io::PacketBuffers;
//! use aeronet::ringbuf::traits::{Consumer, Producer};
//!
//! fn print_all_packets(
//!     mut sessions: Query<(Entity, &mut PacketBuffers)>,
//! ) {
//!     for (session, mut packet_bufs) in &mut sessions {
//!         for packet in packet_bufs.recv.pop_iter() {
//!             info!("Received packet from {session:?}: {packet:?}");
//!         }
//!
//!         info!("Sending out OK along {session:?}");
//!         packet_bufs.send.push_overwrite(b"OK"[..].into());
//!     }
//! }
//! ```
//!
//! Sent packets must have a length smaller than or equal to [`PacketMtu`],
//! otherwise the packet may be discarded, and a warning may be logged (this is
//! up to the implementation).
//!
//! [session]: crate::session
//! [transport layer]: crate::transport

use std::num::Saturating;

use bevy_app::prelude::*;
use bevy_derive::Deref;
use bevy_ecs::prelude::*;
use bevy_reflect::prelude::*;
use bytes::Bytes;
use derive_more::{Add, AddAssign, Sub, SubAssign};
use ringbuf::HeapRb;

use crate::session::Session;

#[derive(Debug)]
pub struct IoPlugin;

impl Plugin for IoPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(PreUpdate, IoSet::Poll)
            .configure_sets(PostUpdate, IoSet::Flush)
            .register_type::<PacketMtu>()
            .register_type::<IoStats>()
            .observe(connecting);
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
/// packets from [`PacketBuffers::recv`] often enough using [`pop_iter`], or
/// buffer too many packets into [`PacketBuffers::send`], then you will lose
/// some packets.
///
/// You can think of the capacity of each buffer in this struct as an upper
/// bound on how many packets we can send and receive per [`Update`]. However,
/// the actual capacity is chosen effectively arbitrarily, since we have no
/// hints on how many packets we will be sending/receiving. It's better to
/// overestimate the capacity and allocate some extra memory which is never used
/// rather than to underestimate and drop some packets.
///
/// [session]: crate::session
/// [IO layer]: crate::io
/// [`pop_iter`]: ringbuf::traits::Consumer::pop_iter
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
}

/// Default capacity for the size of the buffers in [`PacketBuffers`].
pub const PACKET_BUF_CAP: usize = 64;

impl Default for PacketBuffers {
    fn default() -> Self {
        Self::new(PACKET_BUF_CAP)
    }
}

/// Maximum transmissible unit (packet length) of outgoing packets on a
/// [session].
///
/// Sent packets must have a length smaller than or equal to this value.
///
/// This component must only be mutated by the IO layer.
///
/// [session]: crate::session
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deref, Component, Reflect)]
#[reflect(Component)]
pub struct PacketMtu(pub usize);

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Component, Reflect, Add, AddAssign, Sub, SubAssign,
)]
#[reflect(Component)]
pub struct IoStats {
    pub packets_recv: Saturating<usize>,
    pub packets_sent: Saturating<usize>,
    pub bytes_recv: Saturating<usize>,
    pub bytes_sent: Saturating<usize>,
}

// TODO: required component on Session
fn connecting(trigger: Trigger<OnAdd, Session>, mut commands: Commands) {
    let session = trigger.entity();
    commands.entity(session).add(|mut entity: EntityWorldMut| {
        if !entity.contains::<PacketBuffers>() {
            entity.insert(PacketBuffers::default());
        }
    });
}
