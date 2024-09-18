//! Layer for handling reliability, ordering, and fragmentation on top of the
//! [IO layer].
//!
//! In most cases, you will want to interact with the transport layer, or some
//! higher-level layer, rather than the [IO layer] itself. This is because the
//! transport layer can provide guarantees on messages being delivered, and the
//! order in which they're delivered, which packets can't.
//!
//! # Messages
//!
//! A message is an arbitrary sequence of bytes which may be of any length,
//! similar to a packet. However, a transport provides features and guarantees
//! for sending and receiving messages which the IO layer does not provide.
//! Notably, messages provide the following:
//! - fragmentation: large messages will be split up into smaller fragments so
//!   that they may fit into a packet, which will be reassembled back into a
//!   single message by the peer
//! - reliability: messages that are sent are guaranteed to be received by the
//!   peer eventually - see [`SendMode`]
//! - ordering: messages will be received in the order that they were sent by
//!   the peer - see [`SendMode`]
//! - acknowledgements: you can be notified when the peer confirms that it has
//!   received one of your sent messages - see [`MessageKey`]
//!
//! # Sending and receiving
//!
//! ```
//! use bevy::prelude::*;
//! use aeronet::transport::MessageBuffers;
//!
//! fn print_all_messages(
//!     mut sessions: Query<(Entity, &mut MessageBuffers)>,
//! ) {
//!     for (session, mut msg_bufs) in &mut sessions {
//!         for msg in msg_bufs.recv.drain(..) {
//!             info!("Received message from {session:?}: {msg:?}");
//!         }
//!
//!         for msg_key in msg_bufs.acks.drain(..) {
//!             info!("Received ack from {session:?} for message {msg_key:?}");
//!         }
//!
//!         info!("Sending out OK along {session:?}");
//!         msg_bufs.send.push(b"OK"[..].into());
//!     }
//! }
//! ```
//!
//! Sent messages must have a length smaller than or equal to [`MessageMtu`],
//! otherwise:
//! - if the message is sent [reliably], the session must be disconnected, as
//!   the reliabiliy guarantee has been broken
//! - if the message is not sent [reliably], it may be discarded, and a warning
//!   may be logged (this is up to the implementation)
//!
//! [IO layer]: crate::io
//! [reliably]: crate::message::SendReliability::Reliable
// TODO how does sending work with message keys?

use std::num::Saturating;

use bevy_app::prelude::*;
use bevy_derive::Deref;
use bevy_ecs::prelude::*;
use bevy_reflect::prelude::*;
use bytes::Bytes;
use derive_more::{Add, AddAssign, Sub, SubAssign};

use crate::{
    io::IoSet,
    message::{MessageKey, SendMode},
};

#[derive(Debug)]
pub(crate) struct TransportPlugin;

impl Plugin for TransportPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(PreUpdate, TransportSet::Poll.after(IoSet::Poll))
            .configure_sets(PostUpdate, TransportSet::Flush.before(IoSet::Flush))
            .register_type::<MessageBuffers>()
            .register_type::<MessageMtu>()
            .register_type::<TransportStats>();
    }
}

/// Set for scheduling [transport layer] systems.
///
/// [transport layer]: crate::transport
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum TransportSet {
    /// Reading packets from the IO layer and converting them into messages.
    ///
    /// By default, this happens after [`IoSet::Poll`].
    Poll,
    /// Converting buffered messages into packets and sending them to the IO
    /// layer.
    ///
    /// By default, this happens before [`IoSet::Flush`].
    Flush,
}

/// Buffers for incoming and outgoing messages on a [session], and incoming
/// message acknowledgements.
///
/// See the [transport layer].
///
/// [session]: crate::session
/// [transport layer]: crate::transport
#[derive(Debug, Clone, Default, Component, Reflect)]
#[reflect(Component)]
pub struct MessageBuffers {
    /// Buffer of messages received from the transport layer during
    /// [`TransportSet::Recv`].
    ///
    /// If this buffer is not drained regularly, it will grow unbounded.
    ///
    /// Each packet in this buffer may be of arbitrary size - it may be 0 bytes
    /// or larger than the [`MessageMtu`] on this session.
    #[reflect(ignore)]
    pub recv: Vec<Bytes>,
    /// Buffer of packets that will be drained and sent out to the transport
    /// layer during [`TransportSet::Send`], along with what [`SendMode`] they
    /// are sent out with.
    ///
    /// Pushing into this buffer is the most efficient way to enqueue messages
    /// for sending, but you will not be able to access the [`MessageKey`] of
    /// any messages that you send.
    /// If you want to get the [`MessageKey`], !!! TODO how? !!!
    ///
    /// Each message pushed into this buffer must have a length smaller than or
    /// equal to [`MessageMtu`].
    #[reflect(ignore)]
    pub send: Vec<(SendMode, Bytes)>,
    /// Buffer of message acknowledgements received from the peer during
    /// [`TransportSet::Recv`].
    ///
    /// If this buffer is not drained regularly, it will grow unbounded.
    #[reflect(ignore)]
    pub acks: Vec<MessageKey>,
}

/// Maximum transmissible unit (message length) of outgoing messages on a
/// [session].
///
/// Sent messages must have a length smaller than or equal to this value.
///
/// [session]: crate::session
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Deref, Component, Reflect,
)]
#[reflect(Component)]
pub struct MessageMtu(pub usize);

/// Statistics for the [transport layer] of a [session].
///
/// As a component, these represent the total values since this session was
/// spawned.
///
/// [transport layer]: crate::transport
/// [session]: crate::session
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Component, Reflect, Add, AddAssign, Sub, SubAssign,
)]
#[reflect(Component)]
pub struct TransportStats {
    /// Number of messages received into [`MessageBuffers::recv`].
    pub msgs_recv: Saturating<usize>,
    /// Number of messages sent out from [`MessageBuffers::send`].
    pub msgs_sent: Saturating<usize>,
    /// Number of message keys received into [`MessageBuffers::acks`].
    pub acks_recv: Saturating<usize>,
}
