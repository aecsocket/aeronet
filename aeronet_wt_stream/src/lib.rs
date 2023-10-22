pub use aeronet_wt_stream_derive::{OnStream, Stream};

/// A side-agnostic type representing a kind of stream used for data transport.
///
/// Different streams are used to trade off reliability and ordering for speed
/// of sending a message.
/// The simplest type of "stream" - a [`StreamKind::Datagram`] is useful for
/// cases when it is OK if a few messages are lost or sent in the wrong order,
/// as they can be sent with little overhead, such as a player's position
/// update.
/// A proper stream like [`StreamKind::Bi`] can be used for cases in which
/// sending data should be ordered and reliable, at the cost of the message
/// potentially being received later, such as a chat message or an interaction
/// event.
/// Multiple streams of the same type can be opened to avoid head-of-line
/// blocking, where a stream is stuck waiting for a message that was sent ages
/// ago, disallowing any new messages to be received until that one is
/// received.
///
/// WebTransport uses the QUIC protocol internally, which allows using multiple
/// streams over the same connection. This type represents which of these
/// streams is used to transport some data.
///
/// There may be multiple streams of the same type open on a single connection,
/// which is why the extra [`StreamId`] field is used - to identify which
/// specific stream this object refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StreamKind {
    Datagram,
    Uni,
    Bi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StreamId {
    Datagram,
    Uni(usize),
    Bi(usize),
}

impl From<StreamId> for StreamKind {
    fn from(value: StreamId) -> Self {
        match value {
            StreamId::Datagram => Self::Datagram,
            StreamId::Uni(_) => Self::Uni,
            StreamId::Bi(_) => Self::Bi,
        }
    }
}

pub trait Stream {
    fn stream_id(&self) -> StreamId;

    fn num_uni() -> usize;

    fn num_bi() -> usize;
}

pub trait OnStream<S> {
    fn on_stream(&self) -> S;
}
