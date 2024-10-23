#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]
#![allow(missing_docs, dead_code)] // TODO

pub mod frag;
pub mod lane;
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
    aeronet_io::{packet::PacketBuffers, IoSet},
    bevy_app::prelude::*,
    bevy_ecs::{prelude::*, schedule::SystemSet},
    bevy_reflect::prelude::*,
    derive_more::{Add, AddAssign, Sub, SubAssign},
    lane::{LaneIndex, LaneKind},
    packet::{Acknowledge, MessageSeq},
    rtt::RttEstimator,
    typesize::{derive::TypeSize, TypeSize},
};

#[derive(Debug)]
pub struct AeronetTransportPlugin;

impl Plugin for AeronetTransportPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(PreUpdate, (IoSet::Poll, TransportSet::Poll).chain())
            .configure_sets(PostUpdate, (TransportSet::Flush, IoSet::Flush).chain())
            .add_systems(PostUpdate, flush.in_set(TransportSet::Flush));
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
    pub send_bytes_per_sec: usize,

    // shared
    // flushed_packets: SeqBuf<FlushedPacket, 1024>,
    stats: MessageStats,
    acks: Acknowledge,

    // recv
    recv_lanes: Box<[recv::Lane]>,
    rtt: RttEstimator,
    pub recv: recv::TransportRecv,

    // send
    pub send: send::TransportSend,
}

impl Transport {
    #[must_use]
    pub fn new(
        recv_lanes: impl IntoIterator<Item = impl Into<LaneKind>>,
        send_lanes: impl IntoIterator<Item = impl Into<LaneKind>>,
    ) -> Self {
        todo!()
        // Self {
        //     recv_lanes: Box::new(()),
        //     send_lanes: Box::new(()),
        //     max_memory_usage: 4 * 1024 * 1024,
        //     send_bytes_per_sec: usize::MAX,
        //     stats: MessageStats::default(),
        //     rtt: RttEstimator::default(),
        // }
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

#[derive(Debug, Clone, Copy, Default, TypeSize, Reflect)] // force `#[derive]` on multiple lines
#[derive(Add, AddAssign, Sub, SubAssign)]
pub struct MessageStats {
    pub msgs_recv: sized::Saturating<usize>,
    pub msgs_sent: sized::Saturating<usize>,
    pub packet_acks_recv: sized::Saturating<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct FragmentPath {
    lane_index: LaneIndex,
    msg_seq: MessageSeq,
    frag_index: usize,
}
