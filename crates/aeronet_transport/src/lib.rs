#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]
#![allow(missing_docs, dead_code)] // TODO

pub mod lane;
pub mod message;
pub mod msg;
pub mod packet;
pub mod rtt;
pub mod seq_buf;

#[cfg(feature = "stats")]
pub mod stats;

#[cfg(feature = "visualizer")]
pub mod visualizer;

pub use {aeronet_io as io, octs};
use {
    ahash::AHashMap,
    bevy_app::prelude::*,
    bevy_ecs::{prelude::*, schedule::SystemSet},
    lane::{LaneKind, LaneReliability},
    message::MessageStats,
    packet::{Acknowledge, MessageSeq},
    rtt::RttEstimator,
    seq_buf::SeqBuf,
    web_time::Instant,
};

#[derive(Debug)]
pub struct AeronetTransportPlugin;

impl Plugin for AeronetTransportPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(PreUpdate, TransportSet::Poll)
            .configure_sets(PostUpdate, TransportSet::Flush)
            .add_plugins(message::MessagePlugin)
            .observe(on_transport_added);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum TransportSet {
    Poll,
    Flush,
}

#[derive(Debug, Component)]
pub struct Transport {
    // config
    pub max_memory_usage: usize,
    pub send_bytes_per_sec: usize,

    // shared
    flushed_packets: SeqBuf<FlushedPacket, 1024>,
    acks: Acknowledge,
    stats: MessageStats,

    // recv
    recv_lanes: Box<RecvLane>,
    rtt: RttEstimator,

    // send
    send_lanes: Box<SendLane>,
}

impl Transport {
    #[must_use]
    pub fn new(
        recv_lanes: impl IntoIterator<Item = impl Into<LaneKind>>,
        send_lanes: impl IntoIterator<Item = impl Into<LaneKind>>,
    ) -> Self {
        Self {
            recv_lanes: Box::new(()),
            send_lanes: Box::new(()),
            max_memory_usage: 4 * 1024 * 1024,
            send_bytes_per_sec: usize::MAX,
            stats: MessageStats::default(),
            rtt: RttEstimator::default(),
        }
    }

    #[must_use]
    pub fn with_max_memory_usage(self, max_memory_usage: usize) -> Self {
        Self {
            max_memory_usage,
            ..self
        }
    }

    #[must_use]
    pub fn with_send_bytes_per_sec(self, send_bytes_per_sec: usize) -> Self {
        Self {
            send_bytes_per_sec,
            ..self
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
}

struct FlushedPacket {
    flushed_at: Instant,
    frags: Box<[FragmentPath]>,
}

struct RecvLane {}

struct SendLane {
    sent_msgs: AHashMap<MessageSeq, SentMessage>,
    next_msg_seq: MessageSeq,
    reliability: LaneReliability,
}

struct SentMessage {
    frags: Box<[Option<SentFragment>]>,
}

struct SentFragment {}
