mod error;
mod stats;

pub use {error::*, stats::*};

use aeronet::{lane::LaneMapper, message::BytesMapper, protocol::TransportProtocol};
use aeronet_proto::seq::Seq;

pub const DEFAULT_MTU: usize = 1200;

pub const DEFAULT_BANDWIDTH: usize = usize::MAX;

/// Key uniquely identifying a message sent from a [`WebTransportclient`] or
/// [`WebTransportServer`].
///
/// [`WebTransportclient`]: crate::client::WebTransportClient
/// [`WebTransportServer`]: crate::server::WebTransportServer
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MessageKey {
    msg_seq: Seq,
}

impl MessageKey {
    /// Creates a message key from a raw message sequence number.
    pub fn from_raw(msg_seq: Seq) -> Self {
        Self { msg_seq }
    }

    /// Gets the underlying message sequence number from this key.
    pub fn into_raw(self) -> Seq {
        self.msg_seq
    }
}

pub trait WebTransportProtocol: TransportProtocol {
    type Mapper: Clone
        + BytesMapper<Self::C2S>
        + BytesMapper<Self::S2C>
        + LaneMapper<Self::C2S>
        + LaneMapper<Self::S2C>;
}
