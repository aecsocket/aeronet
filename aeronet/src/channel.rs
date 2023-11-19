/// Represents what kind of method is used to transport data along a connection.
///
/// A connection may support different methods for transporting messages, where
/// each different method is called a channel. A channel provides guarantees on:
/// * **reliablity** - ensuring that the message reaches the other side without
///   being lost
/// * **ordering** - ensuring that messages are received in the same order that
///   they are sent
///
/// Although it is not a part of the guarantees laid out by the channel kinds,
/// **head-of-line blocking**
///
/// The transport implementation is guaranteed to provide these channel kinds,
/// either by using a feature of the underlying transport mechanism (i.e. QUIC
/// streams on WebTransport) or via a custom layer implemented on top of the
/// transport mechanism.
///
/// Note that channel kinds provide a *minimum* guarantee of reliability and
/// ordering - a transport may provide some guarantees even if using a less
/// reliable channel kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChannelKind {
    /// No guarantees given on **reliability** or **ordering**.
    ///
    /// This is useful for messages which should be sent in a fire-and-forget
    /// manner: that is, you don't expect to get a response for this message,
    /// and it is OK if a few messages are lost in transit.
    ///
    /// This channel kind typically has the best performance, as it does not
    /// require any sort of handshaking to ensure that messages have arrived
    /// from one side to the other.
    ///
    /// An example of a message using this channel kind is a player positional
    /// update, sent to the server whenever a client moves in a game world.
    /// Since the game client will constantly be sending positional update
    /// messages at a high rate, it is OK if a few are lost in transit, as the
    /// server will hopefully catch the next messages.
    Unreliable,
    /// Messages are sent **reliably** but the **ordering** is not guaranteed.
    ///
    /// This is useful for important one-off events where you need a guarantee
    /// that the message will be delivered, but the order in which it is
    /// delivered is not important.
    ///
    /// This channel kind is typically slower to send and receive than an
    /// unreliable message, but is still faster than an ordered channel because
    /// the implementation may be able to avoid head-of-line blocking.
    ///
    /// An example of a message using this channel kind is sending level data
    /// from a server to a client. It is not important what order the different
    /// parts of the level are received in, but it is important that they are
    /// all received.
    ReliableUnordered,
    /// Messages are sent **reliablity** and **ordered**.
    ///
    /// This is useful for important one-off events where you need a guarantee
    /// that the message will be delivered, and the order in which it's
    /// delivered is important.
    ///
    /// This channel kind offers the most guarantees, but is typically slower to
    /// send and receive than other channel kinds. Most notably, implementations
    /// may suffer from head-of-line blocking.
    ///
    /// Implementations may suffer from head-of-line blocking if a reliable
    /// channel is used, where messages cannot be received because they are
    /// being held up by a message sent earlier. To avoid this, you may use
    /// multiple different instances of this kind of channel, all of which hold
    /// their own message queues.
    ///
    /// An example of a message using this channel kind is sending chat messages
    /// from the server to the client. Since the server aggregates chat messages
    /// from different sources (system, other players, etc.) in a specific
    /// order, it must then tell its clients about the chat messages in that
    /// specific order as well.
    ReliableOrdered,
}

/// Represents a finite set of channels that may be opened by an app.
///
/// When you want to send a message from your app, you may need to specify along
/// what channel it is sent. A type that implements this trait effectively acts
/// as this specifier, letting you define a list of all channels that your app
/// will transport over in a single place, and let your code use this throughout
/// the app.
///
/// # Safety
///
/// This should be derived rather than implemented manually - see
/// [`aeronet_derive::ChannelKey`]. Otherwise, transport implementations may
/// panic.
pub unsafe trait ChannelKey: Send + Sync + 'static {
    /// The set of all kinds of channels that this type may represent.
    const ALL: &'static [ChannelKind];

    /// Gets the index in [`Channels::ALL`] that this channel variant maps to.
    ///
    /// # Safety
    ///
    /// * The same value of `self` should always correspond to the same index.
    /// * Different values of `self` must correspond to a unique index.
    /// * The index returned must not be equal to or greater than the length of
    ///   [`Channels::ALL`].
    fn index(&self) -> usize;
}

/// A type which can be sent along a specific variant of a [`ChannelKey`].
///
/// This should be dereived - see [`aeronet_derive::OnChannel`].
///
/// Note that this trait only determines along which channel an *outgoing*
/// message is sent; *incoming* messages are simply received without any
/// channel data.
pub trait OnChannel {
    /// The type of channel key that [`OnChannel::channel_key`] returns.
    type Channel: ChannelKey;

    /// The channel key along which this message is sent.
    fn channel(&self) -> Self::Channel;
}
