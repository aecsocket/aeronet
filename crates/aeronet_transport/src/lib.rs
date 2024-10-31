#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]
#![allow(missing_docs, dead_code)] // TODO

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
    aeronet_io::IoSet,
    bevy_app::prelude::*,
    bevy_ecs::{prelude::*, schedule::SystemSet},
    derive_more::{Add, AddAssign, Sub, SubAssign},
    lane::{LaneIndex, LaneKind},
    limit::TokenBucket,
    packet::{Acknowledge, MessageSeq, PacketSeq},
    rtt::RttEstimator,
    seq_buf::SeqBuf,
    typesize::{derive::TypeSize, TypeSize},
    web_time::Instant,
};

#[derive(Debug)]
pub struct AeronetTransportPlugin;

impl Plugin for AeronetTransportPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(PreUpdate, (IoSet::Poll, TransportSet::Poll).chain())
            .configure_sets(PostUpdate, (TransportSet::Flush, IoSet::Flush).chain())
            .add_systems(PostUpdate, send::flush.in_set(TransportSet::Flush));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum TransportSet {
    Poll,
    Flush,
}

#[derive(Debug, Component, TypeSize)]
pub struct Transport {
    // config
    pub max_memory_usage: usize,

    // shared
    flushed_packets: SeqBuf<FlushedPacket, 1024>,
    stats: MessageStats,
    acks: Acknowledge,

    // recv
    recv_lanes: Box<[recv::Lane]>,
    rtt: RttEstimator,
    pub recv: recv::TransportRecv,

    // send
    bytes_left: TokenBucket,
    next_packet_seq: PacketSeq,
    pub send: send::TransportSend,
}

impl Transport {
    #[must_use]
    pub fn new(
        now: Instant,
        mtu: usize,
        recv_lanes: impl IntoIterator<Item = impl Into<LaneKind>>,
        send_lanes: impl IntoIterator<Item = impl Into<LaneKind>>,
    ) -> Self {
        let max_frag_len = 0;
        Self {
            max_memory_usage: 4 * 1024 * 1024,
            //
            flushed_packets: SeqBuf::new_from_fn(|_| FlushedPacket::new(now)),
            stats: MessageStats::default(),
            acks: Acknowledge::default(),
            //
            recv_lanes: recv_lanes
                .into_iter()
                .map(Into::into)
                .map(|kind| recv::Lane { frags: todo!() })
                .collect(),
            rtt: RttEstimator::default(),
            recv: recv::TransportRecv::new(),
            //
            bytes_left: TokenBucket::new(usize::MAX),
            next_packet_seq: PacketSeq::default(),
            send: send::TransportSend::new(max_frag_len, send_lanes),
        }
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
    pub fn memory_usage(&self) -> usize {
        self.get_size()
    }
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
    frag_index: usize,
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
