#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

pub mod frag;
pub mod lane;
pub mod limit;
pub mod packet;
pub mod recv;
pub mod rtt;
pub mod send;
pub mod seq_buf;
pub mod sized;

#[cfg(feature = "sampling")]
pub mod sampling;

#[cfg(feature = "visualizer")]
pub mod visualizer;

pub use aeronet_io as io;
use {
    aeronet_io::{IoSet, Session, connection::Disconnect, packet::MtuTooSmall},
    arbitrary::Arbitrary,
    bevy_app::prelude::*,
    bevy_ecs::{prelude::*, schedule::SystemSet},
    bevy_reflect::Reflect,
    bevy_time::{Real, Time},
    derive_more::{Add, AddAssign, Sub, SubAssign},
    lane::{LaneIndex, LaneKind},
    limit::TokenBucket,
    octs::FixedEncodeLenHint,
    packet::{Acknowledge, FragmentHeader, FragmentIndex, MessageSeq, PacketHeader, PacketSeq},
    recv::TransportRecv,
    rtt::RttEstimator,
    send::TransportSend,
    seq_buf::SeqBuf,
    tracing::warn,
    typesize::{TypeSize, derive::TypeSize},
    web_time::Instant,
};

/// Sets up the transport layer functionality.
///
/// See [`Transport`].
#[derive(Debug)]
pub struct AeronetTransportPlugin;

impl Plugin for AeronetTransportPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(PreUpdate, (IoSet::Poll, TransportSet::Poll).chain())
            .configure_sets(PostUpdate, (TransportSet::Flush, IoSet::Flush).chain())
            .add_systems(
                PreUpdate,
                (
                    clear_recv_buffers.before(TransportSet::Poll),
                    (
                        recv::poll,
                        update_config,
                        refill_send_bytes,
                        check_memory_limit,
                    )
                        .chain()
                        .in_set(TransportSet::Poll),
                ),
            )
            .add_systems(PostUpdate, send::flush.in_set(TransportSet::Flush))
            .observe(init_config);
    }
}

#[derive(Debug, Component, TypeSize)]
// TODO: required component TransportConfig
pub struct Transport {
    // shared
    flushed_packets: SeqBuf<FlushedPacket, 1024>,
    stats: MessageStats,
    peer_acks: Acknowledge,

    // recv
    recv_lanes: Box<[recv::Lane]>,
    rtt: RttEstimator,
    /// Buffer of received messages.
    ///
    /// This must be drained by the user on every update.
    pub recv_msgs: TransportRecv<RecvMessage>,
    /// Buffer of received message acknowledgements for messages previously
    /// sent via [`TransportSend::push`].
    ///
    /// This must be drained by the user on every update.
    pub recv_acks: TransportRecv<MessageKey>,

    // send
    send_bytes_bucket: TokenBucket,
    next_packet_seq: PacketSeq,
    /// Allows enqueueing messages to be sent along this transport.
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
    /// connections fail with an out-of-memory error, or you see memory usage
    /// is too high in your app.
    pub max_memory_usage: usize,
    /// How many packet bytes we can flush out to the IO layer per second.
    ///
    /// This can be used to limit the outgoing bandwidth of this transport.
    ///
    /// By default, this is [`usize::MAX`].
    pub send_bytes_per_sec: usize,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            max_memory_usage: 4 * 1024 * 1024,
            send_bytes_per_sec: usize::MAX,
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
    pub recv_at: sized::Instant,
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
    ///     tracing::warn,
    ///     web_time::Instant,
    /// };
    ///
    /// const LANES: [LaneKind; 1] = [LaneKind::ReliableOrdered];
    ///
    /// fn on_connected(
    ///     trigger: Trigger<OnAdd, Session>,
    ///     sessions: Query<&Session>,
    ///     mut commands: Commands,
    /// ) {
    ///     let entity = trigger.entity();
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
            //
            recv_lanes: recv_lanes
                .into_iter()
                .map(Into::into)
                .map(recv::Lane::new)
                .collect(),
            rtt: RttEstimator::default(),
            recv_msgs: TransportRecv::new(),
            recv_acks: TransportRecv::new(),
            //
            send_bytes_bucket: TokenBucket::new(0),
            next_packet_seq: PacketSeq::default(),
            send: TransportSend::new(max_frag_len, send_lanes),
        })
    }

    /// Gets the total stats gathered up to now.
    #[must_use]
    pub const fn stats(&self) -> MessageStats {
        self.stats
    }

    /// Gets the current RTT estimation.
    #[must_use]
    pub const fn rtt(&self) -> &RttEstimator {
        &self.rtt
    }

    /// Gets the [`TokenBucket`] for how many packet bytes we have left for
    /// sending.
    #[must_use]
    pub const fn send_bytes_bucket(&self) -> &TokenBucket {
        &self.send_bytes_bucket
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
/// [`Transport::recv_acks`] to read what messages have received acknowledgement
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Arbitrary, TypeSize)]
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
    /// Number of messages received into [`Transport::recv_msgs`].
    pub msgs_recv: sized::Saturating<usize>,
    /// Number of messages sent out from [`Transport::send`].
    pub msgs_sent: sized::Saturating<usize>,
    /// Number of packet acknowledgements received.
    pub packet_acks_recv: sized::Saturating<usize>,
    /// Number of message acknowledgements received into
    /// [`Transport::recv_acks`].
    pub msg_acks_recv: sized::Saturating<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TypeSize)]
struct FragmentPath {
    lane_index: LaneIndex,
    msg_seq: MessageSeq,
    frag_index: FragmentIndex,
}

#[derive(Debug, Clone, TypeSize)]
struct FlushedPacket {
    flushed_at: sized::Instant,
    frags: Box<[FragmentPath]>,
}

impl FlushedPacket {
    fn new(flushed_at: Instant) -> Self {
        Self {
            flushed_at: sized::Instant(flushed_at),
            frags: Box::new([]),
        }
    }
}

// TODO: required component TransportConfig
fn init_config(
    trigger: Trigger<OnInsert, Transport>,
    with_config: Query<(), With<TransportConfig>>,
    mut commands: Commands,
) {
    let entity = trigger.entity();
    if with_config.get(entity).is_err() {
        commands.entity(entity).insert(TransportConfig::default());
    }
}

/// Clears all [`Transport::recv_msgs`] and [`Transport::recv_acks`] buffers,
/// emitting warnings if there were any items left in the buffers.
///
/// The equivalent for [`Transport::send_msgs`] does not exist, because this
/// crate itself is responsible for draining that buffer.
pub fn clear_recv_buffers(mut sessions: Query<(Entity, &mut Transport)>) {
    for (entity, mut transport) in &mut sessions {
        let len = transport.recv_msgs.0.len();
        if len > 0 {
            warn!(
                "{entity} has {len} received messages which have not been consumed - this \
                 indicates a bug in code above the transport layer"
            );
            transport.recv_msgs.0.clear();
        }

        let len = transport.recv_acks.0.len();
        if len > 0 {
            warn!(
                "{entity} has {len} received acks which have not been consumed - this indicates a \
                 bug in code above the transport layer"
            );
            transport.recv_acks.0.clear();
        }
    }
}

fn check_memory_limit(
    mut commands: Commands,
    sessions: Query<(Entity, &Transport, &TransportConfig)>,
) {
    for (session, transport, config) in &sessions {
        let mem_used = transport.memory_used();
        let mem_max = config.max_memory_usage;
        if mem_used > mem_max {
            warn!("{session} exceeded memory limit, disconnecting - {mem_used} / {mem_max} bytes");
            commands.trigger_targets(Disconnect::new("memory limit exceeded"), session);
        }
    }
}

fn update_config(
    mut sessions: Query<
        (&mut Transport, &TransportConfig),
        Or<(Added<Transport>, Changed<TransportConfig>)>,
    >,
) {
    for (mut transport, config) in &mut sessions {
        transport
            .send_bytes_bucket
            .set_cap(config.send_bytes_per_sec);
    }
}

fn refill_send_bytes(time: Res<Time<Real>>, mut sessions: Query<&mut Transport>) {
    for mut transport in &mut sessions {
        transport
            .send_bytes_bucket
            .refill_portion(time.delta_seconds_f64());
    }
}
