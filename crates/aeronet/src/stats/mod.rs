//! Statistics and metadata components that may be present on a [session] or
//! other session-related entities.
//!
//! [session]: crate::session

mod sample;

use std::{net::SocketAddr, time::Duration};

use bevy_derive::Deref;
use bevy_ecs::prelude::*;
use bevy_reflect::prelude::*;
use web_time::Instant;

pub use sample::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Component, Reflect)]
#[reflect(Component)]
pub struct Rtt {
    latest: Duration,
    smoothed: Duration,
    jitter: Duration,
}

impl Rtt {
    #[must_use]
    pub const fn get(&self) -> Duration {
        self.smoothed
    }

    #[must_use]
    pub fn conservative(&self) -> Duration {
        self.smoothed.max(self.latest)
    }
}

/// Instant at which a [session] connected to its peer.
///
/// This is automatically added to the session when [`Connected`] is added.
///
/// [session]: crate::session
/// [`Connected`]: crate::session::Connected
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deref, Component, Reflect)]
#[reflect(Component)]
pub struct ConnectedAt(pub Instant);

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

/// Remote socket address that this [session] is connected to.
///
/// Sessions which use a network will use an OS socket for communication. This
/// component stores the [`SocketAddr`] of the peer, which this session is
/// connected to.
///
/// This component may not be present in environments where there is no access
/// to OS sockets (i.e. WASM).
///
/// To access the local socket address of a session, see [`LocalAddr`].
///
/// [session]: crate::session
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deref, Component)]
pub struct RemoteAddr(pub SocketAddr);
