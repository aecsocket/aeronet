//! Types for representing QUIC streams and their directions.
//!
//! This module lays out building blocks for the higher level stream APIs, which will depend on
//! whether you are on the client or server side. This is to ensure type safety, as you e.g. cannot
//! send along an S2C channel from the client side.
//!
//! See [`TransportStream`] for an explanation of how the underlying streams work.

/// An identifier for a single instance of an opened stream.
///
/// Since you can open multiple streams of the same type on a connection, we need a way to index
/// which exact stream we're using when sending data. This struct provides an opaque way to
/// represent this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StreamId(pub(crate) usize);

impl StreamId {
    /// Creates a stream ID from a raw index.
    ///
    /// Sending data using a stream ID created from this function may lead to bugs or panics, as
    /// an invalid stream will be used to send data. Prefer the functions on [`StreamDefinitions`]
    /// instead to obtain a stream for your current side.
    pub fn from_raw(raw: usize) -> Self {
        Self(raw)
    }

    /// Gets the raw stream ID from this value.
    pub fn into_raw(self) -> usize {
        self.0
    }
}

/// A side-agnostic type representing a kind of stream used for data transport.
///
/// Different streams are used to trade off reliability and ordering for speed of sending a
/// message.
/// The simplest type of "stream" - a [`TransportStream::Datagram`] is useful for cases
/// when it is OK if a few messages are lost or sent in the wrong order, as they can be sent with
/// little overhead, such as a player's position update.
/// A proper stream like [`TransportStream::Bi`] can be used for cases in which
/// sending data should be ordered and reliable, at the cost of the message potentially being
/// received later, such as a chat message or an interaction event.
/// Multiple streams of the same type can be opened to avoid head-of-line blocking, where a stream
/// is stuck waiting for a message that was sent ages ago, disallowing any new messages to be
/// received until that one is received.
///
/// WebTransport uses the QUIC protocol internally, which allows using multiple streams over the
/// same connection. This type represents which of these streams is used to transport some data.
///
/// There may be multiple streams of the same type open on a single connection, which is why the
/// extra [`StreamId`] field is used - to identify which specific stream this object refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransportStream {
    /// Not really a stream, however we treat it as one for the sake of the API.
    ///
    /// Only one of these "streams" exists in a single connection.
    Datagram,
    /// A bidirectional stream, supporting both client-to-server and server-to-client sending.
    Bi(StreamId),
    /// A unidirectional stream, supporting client-to-server sending.
    UniC2S(StreamId),
    /// A unidirectional stream, supporting server-to-client sending.
    UniS2C(StreamId),
}

/// A stream along which the server can send data.
///
/// See [`TransportStream`] for details.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServerStream {
    /// See [`TransportStream::Datagram`].
    Datagram,
    /// See [`TransportStream::Bi`].
    Bi(StreamId),
    /// See [`TransportStream::UniS2C`].
    Uni(StreamId),
}

impl From<ServerStream> for TransportStream {
    fn from(value: ServerStream) -> Self {
        match value {
            ServerStream::Datagram => Self::Datagram,
            ServerStream::Bi(id) => Self::Bi(id),
            ServerStream::Uni(id) => Self::UniS2C(id),
        }
    }
}

/// A stream along which the client can send data.
///
/// See [`TransportStream`] for details.
pub enum ClientStream {
    /// See [`TransportStream::Datagram`].
    Datagram,
    /// See [`TransportStream::Bi`].
    Bi(StreamId),
    /// See [`TransportStream::UniC2S`].
    Uni(StreamId),
}

impl From<ClientStream> for TransportStream {
    fn from(value: ClientStream) -> Self {
        match value {
            ClientStream::Datagram => Self::Datagram,
            ClientStream::Bi(id) => Self::Bi(id),
            ClientStream::Uni(id) => Self::UniC2S(id),
        }
    }
}

/// Defines which streams will be created when a client/server connection is established.
///
/// When a connection is started, the transport opens all required channels during the establish
/// step. Channels cannot be opened or closed afterwards. This struct defines which channels will
/// be created during this step.
///
/// # Usage
///
/// ```
/// use aeronet_wt_native::TransportStreams;
///
/// let mut streams = TransportStreams::default();
/// let game_data = streams.add_server_bi();
/// let map_data = streams.add_server_uni();
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct TransportStreams {
    pub(crate) bi: usize,
    pub(crate) uni_c2s: usize,
    pub(crate) uni_s2c: usize,
}

impl TransportStreams {
    /// Adds a bidirectional stream on the client side.
    pub fn add_bi_c2s(&mut self) -> ClientStream {
        let i = self.bi;
        self.bi += 1;
        ClientStream::Bi(StreamId(i))
    }

    /// Adds a bidirectional stream on the server side.
    pub fn add_bi_s2c(&mut self) -> ServerStream {
        let i = self.bi;
        self.bi += 1;
        ServerStream::Bi(StreamId(i))
    }

    /// Adds a client-to-server unidirectional stream.
    pub fn add_uni_c2s(&mut self) -> ClientStream {
        let i = self.uni_c2s;
        self.uni_c2s += 1;
        ClientStream::Uni(StreamId(i))
    }

    /// Adds a server-to-client unidirectional stream.
    pub fn add_uni_s2c(&mut self) -> ServerStream {
        let i = self.uni_s2c;
        self.uni_s2c += 1;
        ServerStream::Uni(StreamId(i))
    }
}
