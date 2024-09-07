//! Statistics and metadata components that may be present on a [session] or
//! other session-related entities.
//!
//! [session]: crate::session

use std::{net::SocketAddr, num::Saturating, time::Duration};

use bevy_derive::Deref;
use bevy_ecs::prelude::*;
use bevy_reflect::prelude::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Component, Reflect)]
#[reflect(Component)]
pub struct Rtt {
    latest: Duration,
    smoothed: Duration,
    jitter: Duration,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Component, Reflect)]
#[reflect(Component)]
pub struct SessionStats {
    /// Number of messages (created by the user) sent to the transport
    /// layer.
    pub msgs_sent: Saturating<usize>,
    /// Number of messages received from the transport layer.
    pub msgs_recv: Saturating<usize>,
    /// Number of packets (created by the transport layer) sent to the IO
    /// layer.
    pub packets_sent: Saturating<usize>,
    /// Number of packets received from the IO layer by the transport layer.
    pub packets_recv: Saturating<usize>,
    /// Number of bytes sent out by the IO layer.
    pub bytes_sent: Saturating<usize>,
    /// Number of bytes received by the IO layer.
    pub bytes_recv: Saturating<usize>,
    /// Number of acknowledgements for our packets that have been received
    /// by the peer.
    pub acks_recv: Saturating<usize>,
}

/// Local socket address that this entity uses for connections.
///
/// Sessions or servers which use a network will use an OS socket for
/// communication. This component stores the local [`SocketAddr`] of this
/// socket.
///
/// This component may not be present in environments where there is no access
/// to OS sockets (i.e. WASM).
///
/// To access the remote socket address of a session, see [`RemoteAddr`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deref, Component)]
pub struct LocalAddr(pub SocketAddr);

/// Remote socket address that this session is connected to.
///
/// Sessions which use a network will use an OS socket for communication. This
/// component stores the [`SocketAddr`] of the peer, which this session is
/// connected to.
///
/// This component may not be present in environments where there is no access
/// to OS sockets (i.e. WASM).
///
/// To access the local socket address of a session, see [`LocalAddr`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deref, Component)]
pub struct RemoteAddr(pub SocketAddr);
