use std::time::Duration;

use aeronet::lane::LaneKind;

use crate::byte_bucket::ByteBucket;

/// Configuration of a single outgoing [lane](aeronet::lane).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LaneConfig {
    /// Kind of lane.
    pub kind: LaneKind,
    /// Maximum amount of bytes which can be sent out on this lane per second.
    pub bandwidth: usize,
    pub resend_after: Duration,
}

impl Default for LaneConfig {
    fn default() -> Self {
        Self::new(LaneKind::UnreliableUnordered)
    }
}

impl LaneConfig {
    /// Creates a lane config from the default values and the given lane kind.
    pub const fn new(kind: LaneKind) -> Self {
        Self {
            kind,
            bandwidth: usize::MAX,
            resend_after: Duration::from_millis(100),
        }
    }
}

#[derive(Debug)]
pub enum LaneSender {
    UnreliableUnordered {
        bytes: ByteBucket,
    },
    UnreliableSequenced {
        bytes: ByteBucket,
    },
    ReliableUnordered {
        bytes: ByteBucket,
        resend_after: Duration,
    },
    ReliableOrdered {
        bytes: ByteBucket,
        resend_after: Duration,
    },
}

impl LaneSender {
    /// Creates a new lane sender from the given outgoing lane configuration.
    pub fn new(config: &LaneConfig) -> Self {
        let LaneConfig {
            kind,
            bandwidth,
            resend_after,
        } = config;
        let bandwidth = *bandwidth;
        let resend_after = *resend_after;
        match kind {
            LaneKind::UnreliableUnordered => Self::UnreliableUnordered {
                bytes: ByteBucket::new(bandwidth),
            },
            LaneKind::UnreliableSequenced => Self::UnreliableSequenced {
                bytes: ByteBucket::new(bandwidth),
            },
            LaneKind::ReliableUnordered => Self::ReliableUnordered {
                bytes: ByteBucket::new(bandwidth),
                resend_after,
            },
            LaneKind::ReliableOrdered => Self::ReliableOrdered {
                bytes: ByteBucket::new(bandwidth),
                resend_after,
            },
        }
    }

    /// When sending a message on this lane, what do we do with the message
    /// after sending it?
    pub fn on_send(&self) -> OnSend {
        match self {
            Self::UnreliableUnordered { .. } | Self::UnreliableSequenced { .. } => {
                OnSend::DropMessage
            }
            Self::ReliableUnordered { .. } | Self::ReliableOrdered { .. } => OnSend::RetainMessage,
        }
    }
}

/// What to do with a message after it has been passed to
/// [`LaneSender::on_send`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OnSend {
    /// Message is retained and may be sent again later.
    RetainMessage,
    /// Message is immediately dropped.
    DropMessage,
}
