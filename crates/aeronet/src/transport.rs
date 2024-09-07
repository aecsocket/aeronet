//! Layer for handling reliability, ordering, and fragmentation on top of the
//! [IO layer] using packets.
//!
//! In most cases, you will want to interact with the transport layer, or some
//! higher-level layer, rather than the [IO layer] itself. This is because the
//! IO layer provides no guarantees on if sent packets will be delivered, if
//! they will be received in the right order, or breaking down large blocks of
//! data into smaller ones that fit into packets. These features are the
//! responsibility of the transport layer.
//!
//! The main types used at the IO layer are:
//! - [`MessageBuffers`] - for sending and receiving messages, and receiving
//!   acknowledgements
//! - [`MessageMtu`] - for checking how large one of your sent messages may be
//!
//! # Messages
//!
//! A message is an arbitrary sequence of bytes which may be of any length,
//! similar to a packet. However, a transport provides features and guarantees
//! for sending and receiving messages which the IO layer does not provide.
//! Notably, messages provide the following:
//! - fragmentation - large messages will be split up into smaller fragments so
//!   that they may fit into a packet, which will be reassembled back into a
//!   single message by the peer
//! - reliability - messages that are sent are guaranteed to be received by the
//!   peer eventually - see [`SendMode`]
//! - ordering - messages will be received in the order that they were sent by
//!   the peer - see [`SendMode`]
//! - acknowledgements - you can be notified when the peer confirms that it has
//!   received one of your sent messages - see [`MessageKey`]
//!
//! # Sending and receiving
//!
//! TODO how does this work with message keys?
//!
//! [IO layer]: crate::io

use bevy_app::prelude::*;
use bevy_derive::Deref;
use bevy_ecs::prelude::*;
use bevy_reflect::prelude::*;
use bytes::Bytes;

use crate::{
    io::IoSet,
    message::{MessageKey, SendMode},
};

#[derive(Debug)]
pub struct TransportPlugin;

impl Plugin for TransportPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(PreUpdate, TransportSet::Recv.after(IoSet::Recv))
            .configure_sets(PostUpdate, TransportSet::Send.before(IoSet::Send))
            .register_type::<MessageBuffers>()
            .register_type::<MessageMtu>();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum TransportSet {
    /// Decoding packets received from the IO layer into messages, draining
    /// [`PacketBuffers::recv`] and filling up [`MessageBuffers::recv`].
    ///
    /// [`PacketBuffers::recv`]: crate::io::PacketBuffers::recv
    Recv,
    /// Encoding messages into packets, draining [`MessageBuffers::send`] and
    /// filling up [`PacketBuffers::send`].
    ///
    /// [`PacketBuffers::send`]: crate::io::PacketBuffers::send
    Send,
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
    /// layer during [`TransportSet::Send`].
    ///
    /// Pushing into this buffer is the most efficient way to enqueue messages
    /// for sending, but you will not be able to access the [`MessageKey`] of
    /// any messages that you send.
    /// If you want to get the [`MessageKey`], !!! TODO how? !!!
    ///
    /// Each packet pushed into this buffer must be smaller than or equal to
    /// [`MessageMtu`] in length, otherwise the packet may be discarded.
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
/// Messages pushed into [`MessageBuffers::send`] must have a length smaller
/// than or equal to this value.
///
/// [session]: crate::session
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deref, Component, Reflect)]
#[reflect(Component)]
pub struct MessageMtu(pub usize);
