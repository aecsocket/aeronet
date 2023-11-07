#![warn(clippy::all)]
#![warn(missing_docs)]
#![doc = include_str!("../README.md")]

pub use aeronet_wt_core_derive::*;

/// A side-agnostic type representing a kind of method used for data transport.
///
/// See the [crate docs](crate) and variant docs for info.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChannelKind {
    /// Sends and receives messages unreliably and unordered in a
    /// fire-and-forget manner.
    ///
    /// This is the simplest type of channel and does not require a stream
    /// to be opened beforehand to use.
    /// * not **reliable**
    /// * no guaranteed **ordering**
    /// * no **head-of-line blocking**
    ///
    /// This may be used in cases when you are sending a small bit of
    /// unimportant data which you don't expect a response for, and it is OK
    /// if some is lost in transit e.g. player movement data.
    Datagram,
    /// Sends and receives messages in a reliable and ordered manner.
    ///
    /// This uses a bidirectional stream under the hood to provide message
    /// transport along a QUIC stream. To use a stream, one must be opened
    /// in advance.
    /// * **reliable**
    /// * guaranteed **ordering**
    /// * suffers from **head-of-line blocking**
    ///
    /// This may be used for data which you expect a response for, or one-off
    /// events which are important e.g. a player's chat message or level data.
    ///
    /// To avoid head-of-line blocking, multiple instances of a stream may
    /// exist on the same connection. However, this type cannot represent the
    /// difference between them. If you need this feature, see [`ChannelId`].
    Stream,
}

/// A side-agnostic type representing an instance of a method used for data
/// transport.
///
/// See the [crate docs](crate) for info.
///
/// This type allows specifying exactly what instance of [`ChannelId::Stream`]
/// this value represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChannelId {
    /// See [`ChannelKind::Datagram`].
    Datagram,
    /// See [`ChannelKind::Stream`].
    ///
    /// The single field on this variant represents the index of the opened
    /// stream, determined by the order in which it was opened compared to
    /// other streams.
    Stream(usize),
}

impl From<ChannelId> for ChannelKind {
    fn from(value: ChannelId) -> Self {
        match value {
            ChannelId::Datagram => Self::Datagram,
            ChannelId::Stream(_) => Self::Stream,
        }
    }
}

/// Holds variants for the different types of channels used by an app.
///
/// # Safety
///
/// This should be derived rather than implemented manually - see
/// [`aeronet_wt_core_derive::Channels`]. Otherwise, transport implementations
/// may panic.
pub unsafe trait Channels: 'static {
    /// The number of different [`ChannelId::Stream`] variants that may be
    /// returned by [`Channels::channel_id`].
    const NUM_STREAMS: usize;

    /// Gets the ID of this channel variant.
    ///
    /// # Safety
    ///
    /// * If this returns a [`ChannelId::Datagram`], this is always safe.
    /// * If this returns a [`ChannelId::Stream`]:
    ///   * this must return a unique value for each variant of this type
    ///   * the index of the stream must be an incrementing integer starting
    ///     from 0
    ///   * the index of the stream must not be equal to or exceed
    ///     [`Channels::NUM_STREAMS`]
    fn channel_id(&self) -> ChannelId;
}

/// A message which is sent on a specific variant of [`Channels`].
///
/// This may be derived - see [`aeronet_wt_core_derive::OnChannel`].
///
/// Note that this trait only determines along which channel an *outgoing*
/// message is sent; *incoming* messages are simply received without any
/// channel data.
pub trait OnChannel {
    /// The type of channel that [`OnChannel::channel`] returns.
    type Channel: Channels;

    /// Gets the channel variant along which this message is sent.
    fn channel(&self) -> Self::Channel;
}
