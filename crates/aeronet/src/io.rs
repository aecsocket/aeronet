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
//! - [`PacketBuffers`] - for sending and receiving packets
//! - [`PacketMtu`] - for checking how large one of your sent packets may be
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
//!
//! fn print_all_packets(
//!     mut sessions: Query<(Entity, &mut PacketBuffers)>,
//! ) {
//!     for (session, mut packet_bufs) in &mut sessions {
//!         for packet in packet_bufs.recv.drain(..) {
//!             info!("Received packet from {session:?}: {packet:?}");
//!         }
//!
//!         info!("Sending out OK along {session:?}");
//!         packet_bufs.send.push(&b"OK"[..].into());
//!     }
//! }
//! ```
//!
//! [session]: crate::session
//! [transport layer]: crate::transport

use bevy_app::prelude::*;
use bevy_derive::Deref;
use bevy_ecs::prelude::*;
use bevy_reflect::prelude::*;
use bytes::Bytes;

#[derive(Debug)]
pub struct IoPlugin;

impl Plugin for IoPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(PreUpdate, IoSet::Recv)
            .configure_sets(PostUpdate, IoSet::Send)
            .register_type::<PacketBuffers>()
            .register_type::<PacketMtu>();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum IoSet {
    /// Receiving packets from the IO layer, filling up [`PacketBuffers::recv`].
    Recv,
    /// Sending packets to the IO layer, draining [`PacketBuffers::send`].
    Send,
}

/// Buffers for incoming and outgoing packets on a [session].
///
/// See the [IO layer].
///
/// [session]: crate::session
/// [IO layer]: crate::io
#[derive(Debug, Clone, Default, Component, Reflect)]
#[reflect(Component)]
pub struct PacketBuffers {
    /// Buffer of packets received from the IO layer during [`IoSet::Recv`].
    ///
    /// If this buffer is not drained regularly, it will grow unbounded.
    ///
    /// Each packet in this buffer may be of arbitrary size - it may be 0 bytes
    /// or larger than the [`PacketMtu`] on this session.
    #[reflect(ignore)]
    pub recv: Vec<Bytes>,
    /// Buffer of packets that will be drained and sent out to the IO layer
    /// during [`IoSet::Send`].
    ///
    /// Each packet pushed into this buffer must be smaller than or equal to
    /// [`PacketMtu`] in length, otherwise the packet may be discarded.
    #[reflect(ignore)]
    pub send: Vec<Bytes>,
}

/// Maximum transmissible unit (packet length) of outgoing packets on a
/// [session].
///
/// Packets pushed into [`PacketBuffers::send`] must have a length smaller than
/// or equal to this value.
///
/// [session]: crate::session
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deref, Component, Reflect)]
#[reflect(Component)]
pub struct PacketMtu(pub usize);
