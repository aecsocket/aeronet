#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

pub mod connection;
pub mod packet;
pub mod server;

pub use {anyhow, bytes, web_time};
use {
    bevy_app::prelude::*,
    bevy_ecs::prelude::*,
    bevy_reflect::prelude::*,
    bytes::Bytes,
    packet::{MtuTooSmall, PacketStats, RecvPacket},
    web_time::Instant,
};

/// Sets up the IO layer functionality.
///
/// See [`Session`].
#[derive(Debug)]
pub struct AeronetIoPlugin;

impl Plugin for AeronetIoPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<Session>()
            .configure_sets(PreUpdate, IoSet::Poll)
            .configure_sets(PostUpdate, IoSet::Flush)
            .add_plugins((
                packet::PacketPlugin,
                connection::ConnectionPlugin,
                server::ServerPlugin,
            ));
    }
}

/// Represents an [`Entity`] which is establishing a connection to a peer, so
/// that it may open a [`Session`] in the future.
///
/// This is effectively a marker component for a [`Session`] which isn't
/// connected yet.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Component, Reflect)]
#[reflect(Component)]
pub struct Endpoint;

/// Represents an [`Entity`] which can be used to transfer [packets] over a
/// connection to a peer session, potentially over a network.
///
/// # Overview
///
/// A session can send data over to the other side of its connection - to its
/// peer. The peer may be located on a different machine, on the same machine as
/// this session, or even within the same app. This data is sent in the form of
/// [packets].
///
/// The session API is agnostic to the networking model used: it can be used to
/// represent a client-server, peer-to-peer, or any other kind of network
/// topology. The only constraint is that one session talks to one and only one
/// peer for its lifetime, however you can have multiple sessions within the
/// same world. These different sessions may even be communicating over
/// different protocols, such as raw UDP datagrams alongside Steam networking
/// sockets, so that you can e.g. support crossplay between different platforms.
///
/// The [`Session`] component is managed by your chosen IO layer implementation,
/// and you should not modify it yourself.
///
/// # Lifecycle
///
/// After creating a session entity using your chosen IO layer, the entity may
/// not start with the [`Session`] component - the session is *connecting* but
/// is not *connected* yet. This connecting state is marked with the\
/// [`Endpoint`] component. Once the IO layer adds [`Session`], the entity is
/// considered *connected*, and you can send and receive data.
///
/// Note that [`Session`] is not a *guarantee* that you can send and receive
/// data - it is always possible that operations on OS sockets fail, the network
/// may be suddenly unreachable, etc.
///
/// If the session fails to connect, or loses connection after successfully
/// connecting (this may be a graceful disconnect or a connection error),
/// [`Disconnected`] is [triggered][trigger] on the session entity, and the
/// session is despawned immediately afterwards. You may also [trigger] your own
/// disconnection with a string reason by triggering [`Disconnect`].
///
/// # Packet buffers
///
/// [`Session`] holds the buffers of incoming and outgoing [packets] in
/// [`Session::recv`] and [`Session::send`] respectively. These buffers are
/// [`Vec`]s with unbounded capacity, but are cleared automatically on every
/// update:
/// - [`packet::clear_recv_buffers`] before [`IoSet::Poll`]
/// - [`packet::clear_send_buffers`] before [`IoSet::Flush`]
///
/// If there are any unconsumed packets in a buffer when it is cleared, a
/// warning is emitted - all packets should be consumed on every update.
///
/// # MTU
///
/// TODO
///
/// [trigger]: Trigger
/// [packets]: packet
#[derive(Debug, Component, Reflect)]
#[reflect(from_reflect = false, Component)]
// TODO: required component Endpoint
pub struct Session {
    connected_at: Instant,
    min_mtu: usize,
    mtu: usize,
    /// Total packet statistics of this session up to now.
    ///
    /// The IO layer is responsible for updating this field.
    pub stats: PacketStats,
    /// Buffer for incoming packets received by the IO layer.
    ///
    /// This should only be pushed into by the IO layer, and drained by code
    /// outside the IO layer.
    #[reflect(ignore)]
    pub recv: Vec<RecvPacket>,
    /// Buffer for outgoing packets to be sent out by the IO layer.
    ///
    /// Each packet in this buffer must be a maximum of [`Session::mtu`] bytes
    /// long, otherwise the packet may not be sent out.
    ///
    /// This should only be pushed into outside the IO layer, and drained by the
    /// IO layer.
    #[reflect(ignore)]
    pub send: Vec<Bytes>,
}

impl Session {
    /// Creates a new [`Session`].
    ///
    /// The [`Session::mtu`] will be set to `min_mtu` by default. If you already
    /// have an initial MTU estimate, use [`Session::set_mtu`] after creation
    /// to provide the estimate.
    ///
    /// - `connected_at`: the instant at which the IO layer acknowledged that
    ///   the session is now connected and ready for transporting data.
    /// - `min_mtu`: the lowest MTU value that this session may possibly ever
    ///   have. This is usually a constant based on the protocol used, e.g.
    ///   "[`IP_MTU`] minus overhead".
    ///
    /// [`IP_MTU`]: packet::IP_MTU
    #[must_use]
    pub fn new(connected_at: Instant, min_mtu: usize) -> Self {
        Self {
            connected_at,
            min_mtu,
            mtu: min_mtu,
            stats: PacketStats::default(),
            recv: Vec::new(),
            send: Vec::new(),
        }
    }

    /// Returns when this session established its connection.
    ///
    /// # Examples
    ///
    /// ```
    /// use {aeronet_io::Session, web_time::Instant};
    ///
    /// let now = Instant::now();
    /// let session = Session::new(now, 1000);
    /// assert_eq!(now, session.connected_at());
    /// ```
    #[must_use]
    pub const fn connected_at(&self) -> Instant {
        self.connected_at
    }

    /// Returns the minimum value that [`Session::mtu`] will ever report on this
    /// session.
    ///
    /// See [`packet`].
    ///
    /// # Examples
    ///
    /// ```
    /// use {aeronet_io::Session, web_time::Instant};
    ///
    /// let session = Session::new(Instant::now(), 1000);
    /// assert_eq!(1000, session.min_mtu());
    /// ```
    #[must_use]
    pub const fn min_mtu(&self) -> usize {
        self.min_mtu
    }

    /// Returns the current packet MTU of this session.
    ///
    /// See [`packet`].
    ///
    /// # Examples
    ///
    /// ```
    /// use {aeronet_io::Session, web_time::Instant};
    ///
    /// let mut session = Session::new(Instant::now(), 1000);
    /// assert_eq!(1000, session.mtu());
    ///
    /// session.set_mtu(1400);
    /// assert_eq!(1400, session.mtu());
    /// ```
    #[must_use]
    pub const fn mtu(&self) -> usize {
        self.mtu
    }

    /// Updates the current packet MTU of this session.
    ///
    /// This must only be called by the IO layer.
    ///
    /// # Errors
    ///
    /// Errors if `mtu` is smaller than [`Session::min_mtu`].
    ///
    /// # Examples
    ///
    /// ```
    /// use {aeronet_io::Session, web_time::Instant};
    ///
    /// let mut session = Session::new(Instant::now(), 1000);
    /// session.set_mtu(1200).unwrap();
    /// assert_eq!(1200, session.mtu());
    ///
    /// session.set_mtu(800).unwrap_err();
    /// ```
    pub fn set_mtu(&mut self, mtu: usize) -> Result<(), MtuTooSmall> {
        if mtu >= self.min_mtu {
            self.mtu = mtu;
            Ok(())
        } else {
            Err(MtuTooSmall {
                mtu,
                min: self.min_mtu,
            })
        }
    }
}

/// Set for scheduling IO layer systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum IoSet {
    /// Progressing the connection, handling disconnects, and receiving packets.
    Poll,
    /// Sending buffered packets.
    Flush,
}
