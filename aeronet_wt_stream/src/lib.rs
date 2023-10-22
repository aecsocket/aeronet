#![warn(clippy::all)]
#![warn(missing_docs)]
#![doc = include_str!("../README.md")]

pub use aeronet_wt_stream_derive::*;

/// A side-agnostic type representing a kind of stream used for data transport.
///
/// WebTransport uses the QUIC protocol internally, which allows using different
/// methods of data transport for different situations, trading off reliability
/// and ordering for speed. These methods are represented in this enum.
///
/// The simplest type of "stream" - a [`StreamKind::Datagram`] - is useful for
/// cases when it is OK if a few messages are lost or sent in the wrong order,
/// as they can be sent with little overhead, such as a player's position
/// update.
///
/// The proper stream type [`StreamKind::Bi`] can be used
/// for cases in which sending data should be ordered and reliable, at the cost
/// of the message potentially being received later, such as a chat message or
/// an interaction event.
///
/// QUIC also supports unidirectional streams, however implementing these
/// heavily complicates the API surface and bidirectional streams are usually a
/// good replacement.
///
/// The connection may have multiple streams of the same type open, however this
/// type cannot represent this. For this use case, see [`StreamId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StreamKind {
    /// Sends and receives messages unreliably and unordered in a
    /// fire-and-forget manner.
    ///
    /// This isn't really a stream, but we treat it as one for API's sake.
    Datagram,
    /// Sends and receives messages in a reliable and ordered manner.
    ///
    /// Multiple instances of a stream may exist, in order to e.g.
    /// avoid head-of-line blocking, however this variant does not represent the
    /// individual stream instance. If you need to represent the individual
    /// stream instance, use [`StreamId`].
    Bi,
}

/// A side-agnostic type representing an instance of a stream used for data
/// transport.
///
/// See [`StreamKind`] for a description of how streams work.
///
/// Multiple streams of the same type can be opened on the same connection to
/// avoid head-of-line blocking, where a stream is stuck waiting for a message
/// that was sent ages ago, disallowing any new messages to be received until
/// that one is received. This type represents a single instance of one of those
/// open streams.
///
/// The extra field on the variants represents the index of this stream
/// instance, in the order that it gets opened by the connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StreamId {
    /// See [`StreamKind::Datagram`].
    Datagram,
    /// See [`StreamKind::Bi`].
    Bi(usize),
}

impl From<StreamId> for StreamKind {
    fn from(value: StreamId) -> Self {
        match value {
            StreamId::Datagram => Self::Datagram,
            StreamId::Bi(_) => Self::Bi,
        }
    }
}

/// Holds variants for the different types of streams used by an app.
///
/// This should be derived - see [`aeronet_wt_stream_derive::Streams`].
/// Otherwise, transport implementations will panic from invalid parameters.
pub trait Streams {
    /// Gets the ID of this stream variant.
    fn stream_id(&self) -> StreamId;

    /// Gets the number of [`StreamKind::Bi`] variants in this type.
    fn num_bi() -> usize;
}

/// A message which is sent on a specific variant of [`Streams`].
///
/// This may be derived - see [`aeronet_wt_stream_derive::OnStream`].
pub trait OnStream<S>
where
    S: Streams,
{
    /// Gets the stream along which this message is sent.
    fn on_stream(&self) -> S;
}
