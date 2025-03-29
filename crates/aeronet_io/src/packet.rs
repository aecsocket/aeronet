//! Transmission of unreliable byte sequences (packets) along a [`Session`].
//!
//! # Packets
//!
//! A packet is a sequence of bytes sent from one [`Session`] to another (its
//! peer).
//!
//! Guarantees:
//! - the contents of the packet will be the same on the receiving side as the
//!   sender side.
//!   - this means the packet may *not* be truncated, extended, or otherwise
//!     corrupted.
//!   - it is the IO layer's responsibility to detect this, attempt to correct
//!     it, or drop the packet.
//!
//! Non-guarantees:
//! - the packet may not be sent.
//! - the packet may not be received by the peer.
//! - the packet may be received multiple times.
//! - the packet may not arrive in any fixed amount of time.
//! - the packet may arrive out of order relative to other packets.
//!
//! # Maximum transmissible unit (MTU)
//!
//! The maximum length of a sent packet is called the maximum transmissible
//! unit. The MTU varies for different IO layers, but all IO layers guarantee
//! that they provide two MTU values:
//! - a current MTU value, which may vary over the lifetime of the session.
//! - a minimum MTU value, which the current MTU value may never drop below, and
//!   which itself may never change *over the lifetime of a session*.
//!   - this does not mean that the minimum MTU is constant for a given kind of
//!     IO layer, just that it may not change after a session is started.
//!
//! This value may even be [`usize::MAX`] (if using e.g. MPSC channels). Code
//! which uses the packet MTU must be resilient to being given any MTU value.
//!
//! A networked transport may choose to use [`IP_MTU`] as a base MTU value,
//! minus protocol overhead.

use {
    crate::{IoSet, Session},
    bevy_app::prelude::*,
    bevy_ecs::prelude::*,
    bevy_platform_support::time::Instant,
    bevy_reflect::prelude::*,
    bytes::Bytes,
    core::{num::Saturating, time::Duration},
    derive_more::{Add, AddAssign, Deref, Display, Error, Sub, SubAssign},
    log::warn,
};

pub(crate) struct PacketPlugin;

impl Plugin for PacketPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<PacketRtt>()
            .register_type::<PacketStats>()
            .add_systems(PreUpdate, clear_recv_buffers.before(IoSet::Poll))
            .add_systems(PostUpdate, clear_send_buffers.after(IoSet::Flush));
    }
}

/// Incoming packet pushed into a [`Session`] by the IO layer.
#[derive(Debug, Clone)]
pub struct RecvPacket {
    /// Earliest instant at which we identified this packet.
    ///
    /// This may be earlier than when the packet was pushed into
    /// [`Session::recv`], if for example the packet was received in an async
    /// task and later forwarded to a [`Session`] in the Bevy world.
    pub recv_at: Instant,
    /// Raw byte data of this packet.
    pub payload: Bytes,
}

/// Safe estimate for the [MTU] of a packet sent using the Internet Protocol.
///
/// If an IO layer sends packets over the internet, it should use this value
/// minus any overhead as its [`Session::min_mtu`].
///
/// Technically, a network path may have an MTU lower than this value. However,
/// this is unlikely, and should probably be treated as a path that cannot be
/// used (therefore a connection cannot be established).
///
/// [MTU]: crate::packet
pub const IP_MTU: usize = 1024;

/// Provided a packet [MTU] which is too small.
///
/// [MTU]: crate::packet
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, Error)]
#[display("packet MTU too small - {mtu} / {min}")]
pub struct MtuTooSmall {
    /// Provided MTU value.
    pub mtu: usize,
    /// Minimum MTU that should be supported.
    pub min: usize,
}

/// Statistics for the IO layer of a [`Session`].
#[derive(Debug, Clone, Copy, Default, Reflect)] // force `#[derive]` on multiple lines
#[derive(Add, AddAssign, Sub, SubAssign)]
pub struct PacketStats {
    /// Number of packets received into [`Session::recv`].
    pub packets_recv: Saturating<usize>,
    /// Number of packets sent out from [`Session::send`].
    pub packets_sent: Saturating<usize>,
    /// Sum of the byte lengths of packets received into
    /// [`Session::recv`].
    pub bytes_recv: Saturating<usize>,
    /// Sum of the byte lengths of packets sent out from
    /// [`Session::send`].
    pub bytes_sent: Saturating<usize>,
}

/// Round-trip time of packets on a [`Session`] as computed by the IO layer.
///
/// At the IO layer, the exact definition of round-trip time is left vague,
/// since this is highly implementation-specific.
///
/// This component may not be present on sessions whose IO layers don't provide
/// an RTT estimate.
///
/// This component must only be mutated by the IO layer.
#[derive(Debug, Clone, Copy, Deref, Component, Reflect)]
#[reflect(Component)]
#[doc(alias = "ping", alias = "latency")]
pub struct PacketRtt(pub Duration);

/// Clears all [`Session::recv`] buffers, emitting warnings if there were any
/// packets left in the buffer.
pub fn clear_recv_buffers(mut sessions: Query<(Entity, &mut Session)>) {
    for (entity, mut session) in &mut sessions {
        let len = session.recv.len();
        if len > 0 {
            warn!(
                "{entity} has {len} received packets which have not been consumed - this \
                 indicates a bug in code above the IO layer"
            );
            session.recv.clear();
        }
    }
}

/// Clears all [`Session::send`] buffers, emitting warnings if there were any
/// packets left in the buffer.
pub fn clear_send_buffers(mut sessions: Query<(Entity, &mut Session)>) {
    for (entity, mut session) in &mut sessions {
        let len = session.send.len();
        if len > 0 {
            warn!(
                "{entity} has {len} sent packets which have not been consumed - this indicates a \
                 bug in the IO layer"
            );
            session.send.clear();
        }
    }
}
