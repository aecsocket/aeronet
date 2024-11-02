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

pub use {aeronet_io as io, octs};
use {
    aeronet_io::{connection::Disconnect, packet::MtuTooSmall, IoSet, Session},
    arbitrary::Arbitrary,
    bevy_app::prelude::*,
    bevy_ecs::{prelude::*, schedule::SystemSet},
    bevy_time::{Real, Time},
    derive_more::{Add, AddAssign, Sub, SubAssign},
    lane::{LaneIndex, LaneKind},
    limit::TokenBucket,
    octs::FixedEncodeLenHint,
    packet::{Acknowledge, FragmentHeader, FragmentIndex, MessageSeq, PacketHeader, PacketSeq},
    rtt::RttEstimator,
    seq_buf::SeqBuf,
    tracing::warn,
    typesize::{derive::TypeSize, TypeSize},
    web_time::Instant,
};

#[derive(Debug)]
pub struct AeronetTransportPlugin;

impl Plugin for AeronetTransportPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(PreUpdate, (IoSet::Poll, TransportSet::Poll).chain())
            .configure_sets(PostUpdate, (TransportSet::Flush, IoSet::Flush).chain())
            .add_systems(
                PreUpdate,
                (
                    recv::poll,
                    update_config,
                    refill_send_bytes,
                    check_memory_limit,
                )
                    .chain()
                    .in_set(TransportSet::Poll),
            )
            .add_systems(PostUpdate, send::flush.in_set(TransportSet::Flush))
            .observe(init_config);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum TransportSet {
    Poll,
    Flush,
}

#[derive(Debug, Clone, Copy, Component, TypeSize)]
pub struct TransportConfig {
    pub max_memory_usage: usize,
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
    pub recv_msgs: recv::TransportRecv<RecvMessage>,
    pub recv_acks: recv::TransportRecv<MessageKey>,

    // send
    send_bytes_bucket: TokenBucket,
    next_packet_seq: PacketSeq,
    pub send: send::TransportSend,
}

#[derive(Debug, TypeSize)]
pub struct RecvMessage {
    pub lane: LaneIndex,
    pub recv_at: sized::Instant,
    pub payload: Vec<u8>,
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

const FRAG_OVERHEAD: usize = PacketHeader::MAX_ENCODE_LEN + FragmentHeader::MAX_ENCODE_LEN;

impl Transport {
    pub fn new(
        session: &Session,
        recv_lanes: impl IntoIterator<Item = impl Into<LaneKind>>,
        send_lanes: impl IntoIterator<Item = impl Into<LaneKind>>,
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
            recv_msgs: recv::TransportRecv::new(),
            recv_acks: recv::TransportRecv::new(),
            //
            send_bytes_bucket: TokenBucket::new(0),
            next_packet_seq: PacketSeq::default(),
            send: send::TransportSend::new(max_frag_len, send_lanes),
        })
    }

    #[must_use]
    pub const fn stats(&self) -> MessageStats {
        self.stats
    }

    #[must_use]
    pub const fn rtt(&self) -> &RttEstimator {
        &self.rtt
    }

    #[must_use]
    pub const fn send_bytes_bucket(&self) -> &TokenBucket {
        &self.send_bytes_bucket
    }

    #[must_use]
    pub fn memory_used(&self) -> usize {
        self.get_size()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Arbitrary, TypeSize)]
pub struct MessageKey {
    pub lane: LaneIndex,
    pub seq: MessageSeq,
}

#[derive(Debug, Clone, Copy, Default, TypeSize)] // force `#[derive]` on multiple lines
#[derive(Add, AddAssign, Sub, SubAssign)]
pub struct MessageStats {
    pub msgs_recv: sized::Saturating<usize>,
    pub msgs_sent: sized::Saturating<usize>,
    pub packet_acks_recv: sized::Saturating<usize>,
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
