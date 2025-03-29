#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]
//!
//! ## Feature flags
#![cfg_attr(feature = "document-features", doc = document_features::document_features!())]
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod frag;
pub mod lane;
pub mod limit;
pub mod min_size;
pub mod packet;
pub mod recv;
pub mod rtt;
pub mod sampling;
pub mod send;
pub mod seq_buf;

#[cfg(feature = "visualizer")]
pub mod visualizer;

pub use aeronet_io as io;
use {
    aeronet_io::{IoSet, Session, connection::Disconnected, packet::MtuTooSmall},
    alloc::{boxed::Box, vec::Vec},
    bevy_app::prelude::*,
    bevy_ecs::{prelude::*, schedule::SystemSet},
    bevy_platform_support::time::Instant,
    bevy_reflect::Reflect,
    core::num::Saturating,
    derive_more::{Add, AddAssign, Display, Error, Sub, SubAssign},
    lane::{LaneIndex, LaneKind},
    log::warn,
    min_size::MinSize,
    octs::FixedEncodeLenHint,
    packet::{Acknowledge, FragmentHeader, MessageSeq, PacketHeader},
    recv::TransportRecv,
    rtt::RttEstimator,
    send::TransportSend,
    seq_buf::SeqBuf,
    typesize::{TypeSize, derive::TypeSize},
};

/// Sets up the transport layer functionality.
///
/// See [`Transport`].
pub struct AeronetTransportPlugin;

impl Plugin for AeronetTransportPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(PreUpdate, (IoSet::Poll, TransportSet::Poll).chain())
            .configure_sets(PostUpdate, (TransportSet::Flush, IoSet::Flush).chain())
            .add_systems(
                PreUpdate,
                (
                    recv::clear_buffers.before(TransportSet::Poll),
                    (
                        recv::poll,
                        send::disconnect_errored,
                        send::update_send_bytes_config,
                        send::refill_send_bytes,
                        check_memory_limit,
                    )
                        .chain()
                        .in_set(TransportSet::Poll),
                ),
            )
            .add_systems(PostUpdate, send::flush.in_set(TransportSet::Flush));
    }
}

/// Manages sending and receiving messages on top of an IO layer.
///
/// The IO layer allows you to send and receive packets (see
/// [`aeronet_io::packet`]), but packets do not provide enough guarantees to be
/// usable for many use cases. [`Transport`] builds on top of packets, allowing
/// you to send and receive messages with more reliabiliy and ordering
/// guarantees via lanes (see [`lane`]).
///
/// # Lifecycle
///
/// After [`Session`] is added to your entity, you should create and add a
/// [`Transport`] component - use a [`Trigger<OnAdd, Session>`] for this. If
/// you are using a crate like `aeronet_replicon`, this step will already be
/// handled for you. You can also add or mutate [`TransportConfig`] at any
/// time during the session lifetime to change its configuration.
///
/// The [`Transport`] itself may disconnect the session prematurely if there
/// is an error such as running out of memory or buffering too many messages.
///
/// # Usage
///
/// - Use [`Transport::send`] to enqueue messages for sending, and to get a key
///   identifying the sent messages
/// - Use [`Transport::recv`]'s [`TransportRecv::msgs`] to drain messages
///   received from the IO layer
/// - Use [`Transport::recv`]'s [`TransportRecv::acks`] to drain message
///   acknowledgements for messages which you have sent out, and that the peer
///   has now acknowledged that they have received.
///
/// The `recv` buffers must be drained on every update, otherwise some may be
/// lost, leading to incorrect behavior, and a warning will be logged.
#[derive(Debug, Component, TypeSize)]
#[require(TransportConfig)]
pub struct Transport {
    // shared
    flushed_packets: SeqBuf<FlushedPacket, 1024>,
    stats: MessageStats,
    peer_acks: Acknowledge,
    rtt: RttEstimator,
    /// Interface to the receiving half of this transport.
    ///
    /// Use this to read received messages and acknowledgements.
    pub recv: TransportRecv,
    /// Interface to the sending half of this transport.
    ///
    /// Use this to enqueue messages to be sent along this transport.
    pub send: TransportSend,
}

/// User-configurable properties of a [`Transport`].
///
/// If you do not provide this component explicitly, a default config will
/// be created and inserted into the [`Session`].
///
/// This component may be modified over the lifetime of a [`Session`] and the
/// [`Transport`] will be updated accordingly.
#[derive(Debug, Clone, Component, TypeSize, Reflect)]
#[reflect(Component)]
pub struct TransportConfig {
    /// Maximum amount of memory, in bytes, that this [`Transport`] may use for
    /// buffering messages until the [`Session`] is forcibly disconnected.
    ///
    /// By default, this is 4 MiB. Consider tuning this number if you see
    /// connections fail with an out-of-memory error, or you see
    /// [`Transport::memory_used`] is too high (you can use the [`visualizer`]
    /// to see real-time statistics).
    pub max_memory_usage: usize,
    /// How many packet bytes we can flush out to the IO layer per second.
    ///
    /// This can be used to limit the outgoing bandwidth of this transport.
    ///
    /// By default, this is [`usize::MAX`].
    pub send_bytes_per_sec: usize,
    /// Multiplier for how long it takes for an unacknowledged packet to be
    /// marked as lost on our side.
    ///
    /// Packet loss is computed by calculating how many packets, that we have
    /// sent out, should have been acknowledged by the peer by now (see
    /// [`SessionStatsSample::loss`]). "By now" is defined as
    /// [`Transport::rtt`]'s [`RttEstimator::pto`], multiplied by this config
    /// value.
    ///
    /// If this value is set too low, or below 1, the packet loss value will be
    /// an overestimate, as the peer will not have enough time to respond with
    /// an acknowledgement.
    /// If this value is set too high, the packet loss value will be an
    /// underestimate, as packets which should be reasonably considered "lost"
    /// will not be marked as such.
    ///
    /// By default, this is 1.5.
    ///
    /// [`SessionStatsSample::loss`]: crate::sampling::SessionStatsSample::loss
    pub packet_lost_threshold_factor: f64,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            max_memory_usage: 4 * 1024 * 1024,
            send_bytes_per_sec: usize::MAX,
            packet_lost_threshold_factor: 1.5,
        }
    }
}

/// Incoming message that a [`Transport`] created from packets received by the
/// IO layer.
#[derive(Debug, TypeSize)]
pub struct RecvMessage {
    /// Lane index on which this message was received.
    pub lane: LaneIndex,
    /// Instant at which the final fragment of this message was received.
    #[typesize(skip)]
    pub recv_at: Instant,
    /// Raw byte data of this message.
    pub payload: Vec<u8>,
}

const FRAG_OVERHEAD: usize = PacketHeader::MAX_ENCODE_LEN + FragmentHeader::MAX_ENCODE_LEN;

impl Transport {
    /// Creates a new transport from an existing [`Session`].
    ///
    /// This should be added to a [`Session`] after it has connected (after the
    /// component is added).
    ///
    /// Also see [`TransportConfig`] for configuration options.
    ///
    /// # Errors
    ///
    /// Errors if the [`Session::min_mtu`] is too small to support messages.
    ///
    /// Since messages take some overhead on the wire (fragmentation, acks,
    /// etc.), packets must be larger than some minimum size to support this.
    ///
    /// # Examples
    ///
    /// ```
    /// use {
    ///     aeronet_io::Session,
    ///     aeronet_transport::{Transport, lane::LaneKind},
    ///     bevy_ecs::prelude::*,
    ///     bevy_platform_support::time::Instant,
    ///     log::warn,
    /// };
    ///
    /// const LANES: [LaneKind; 1] = [LaneKind::ReliableOrdered];
    ///
    /// fn on_connected(
    ///     trigger: Trigger<OnAdd, Session>,
    ///     sessions: Query<&Session>,
    ///     mut commands: Commands,
    /// ) {
    ///     let entity = trigger.target();
    ///     let session = sessions
    ///         .get(entity)
    ///         .expect("we are adding this component to this entity");
    ///     let Ok(transport) = Transport::new(session, LANES, LANES, Instant::now()) else {
    ///         warn!("Failed to create transport for {entity}");
    ///         return;
    ///     };
    ///     commands.entity(entity).insert(transport);
    /// }
    /// ```
    pub fn new(
        session: &Session,
        recv_lanes: impl IntoIterator<Item = LaneKind>,
        send_lanes: impl IntoIterator<Item = LaneKind>,
        now: Instant,
    ) -> Result<Self, MtuTooSmall> {
        let min_mtu = session.min_mtu();
        let max_frag_len = min_mtu.checked_sub(FRAG_OVERHEAD).ok_or(MtuTooSmall {
            mtu: min_mtu,
            min: FRAG_OVERHEAD,
        })?;
        Ok(Self {
            flushed_packets: SeqBuf::new_from_fn(|_| FlushedPacket::new(now)),
            stats: MessageStats::default(),
            peer_acks: Acknowledge::default(),
            rtt: RttEstimator::default(),
            recv: TransportRecv::new(recv_lanes),
            send: TransportSend::new(max_frag_len, send_lanes),
        })
    }

    /// Gets the total stats gathered up to now.
    #[must_use]
    pub const fn stats(&self) -> MessageStats {
        self.stats
    }

    /// Gets access to the RTT estimator, allowing you to read the current RTT
    /// estimates.
    #[must_use]
    pub const fn rtt(&self) -> &RttEstimator {
        &self.rtt
    }

    /// Gets the number of packets that have been flushed out to the peer, but
    /// we have not received an acknowledgement from the peer for them yet.
    #[must_use]
    pub const fn num_unacked_packets(&self) -> usize {
        self.flushed_packets.len()
    }

    /// Gets how many total bytes of memory this transport is using.
    ///
    /// This call is potentially expensive. You should cache this where
    /// possible.
    #[must_use]
    pub fn memory_used(&self) -> usize {
        self.get_size()
    }
}

/// Set for scheduling transport layer systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum TransportSet {
    /// Draining packets from the IO layer and reading out messages; updating
    /// the internal transport state.
    ///
    /// # Ordering
    ///
    /// - [`IoSet::Poll`]
    /// - **[`TransportSet::Poll`]**
    Poll,
    /// Draining messages and turning them into packets for the IO layer.
    ///
    /// # Ordering
    ///
    /// - **[`TransportSet::Flush`]**
    /// - [`IoSet::Flush`]
    Flush,
}

/// Key which pseudo-uniquely identifies a message that has been sent out via
/// [`TransportSend::push`] on this [`Transport`].
///
/// After pushing a message to the transport, you can use the output of
/// [`TransportRecv::acks`] to read what messages have received acknowledgement
/// from the peer. If you see the same message key that you got from
/// [`TransportSend::push`], that message was acknowledged.
///
/// # Uniqueness
///
/// This key is only unique for a certain period of time, which depends on how
/// quickly you send out messages. Internally, this uses [`Seq`] to track the
/// sequence number of messages, but this can and will overflow eventually as
/// long as you keep sending out messages. Therefore, you should not keep
/// message keys around for a long time. As soon as you receive an ack for a
/// message (or don't receive an ack in a certain period of time), drop the
/// key - it's very likely to have the same key as another message later.
///
/// [`Seq`]: packet::Seq
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TypeSize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct MessageKey {
    /// Lane index on which the message was sent.
    pub lane: LaneIndex,
    /// Message sequence number.
    pub seq: MessageSeq,
}

/// Statistics for a [`Transport`].
#[derive(Debug, Clone, Copy, Default, TypeSize)] // force `#[derive]` on multiple lines
#[derive(Add, AddAssign, Sub, SubAssign)]
pub struct MessageStats {
    /// Number of messages received into [`TransportRecv::msgs`].
    pub msgs_recv: Saturating<usize>,
    /// Number of messages sent out from [`Transport::send`].
    pub msgs_sent: Saturating<usize>,
    /// Number of packet acknowledgements received.
    pub packet_acks_recv: Saturating<usize>,
    /// Number of message acknowledgements received into
    /// [`TransportRecv::acks`].
    pub msg_acks_recv: Saturating<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TypeSize)]
struct FragmentPath {
    lane_index: LaneIndex,
    msg_seq: MessageSeq,
    frag_index: MinSize,
}

#[derive(Debug, Clone, TypeSize)]
struct FlushedPacket {
    #[typesize(skip)]
    flushed_at: Instant,
    frags: Box<[FragmentPath]>,
}

impl FlushedPacket {
    fn new(flushed_at: Instant) -> Self {
        Self {
            flushed_at,
            frags: Box::new([]),
        }
    }
}

/// Error in [`Disconnected::ByError`] triggered on a [`Session`] when a
/// [`Transport`] exceeds its memory usage limit.
#[derive(Debug, Clone, Display, Error)]
#[display("memory limit exceeded - {used} / {max} bytes")]
pub struct MemoryLimitExceeded {
    /// Number of bytes used.
    pub used: usize,
    /// Maximum number of bytes the transport is allowed to use.
    pub max: usize,
}

fn check_memory_limit(
    mut commands: Commands,
    sessions: Query<(Entity, &Transport, &TransportConfig)>,
) {
    for (entity, transport, config) in &sessions {
        let mem_used = transport.memory_used();
        let mem_max = config.max_memory_usage;
        if mem_used > mem_max {
            warn!("{entity} exceeded memory limit, disconnecting");
            commands.trigger_targets(
                Disconnected::by_error(MemoryLimitExceeded {
                    used: mem_used,
                    max: mem_max,
                }),
                entity,
            );
        }
    }
}
