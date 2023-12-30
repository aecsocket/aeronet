use std::{fmt::Debug, io, net::SocketAddr, time::Duration};

use aeronet::{
    protocol::PacketError, ByteStats, LaneProtocol, MessageStats, RemoteAddr, Rtt,
    TransportProtocol, TryAsBytes, TryFromBytes,
};
use derivative::Derivative;
use wtransport::{
    error::{ConnectingError, ConnectionError, SendDatagramError, StreamOpeningError},
    Connection,
};

#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    pub remote_addr: SocketAddr,
    pub rtt: Duration,
    pub msgs_sent: usize,
    pub msgs_recv: usize,
    pub bytes_sent: usize,
    pub bytes_recv: usize,
}

impl ConnectionInfo {
    pub fn from_connection(conn: &Connection) -> Self {
        Self {
            remote_addr: conn.remote_address(),
            rtt: conn.rtt(),
            msgs_sent: 0,
            msgs_recv: 0,
            bytes_sent: 0,
            bytes_recv: 0,
        }
    }
}

impl RemoteAddr for ConnectionInfo {
    fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }
}

impl Rtt for ConnectionInfo {
    fn rtt(&self) -> Duration {
        self.rtt
    }
}

impl MessageStats for ConnectionInfo {
    fn msgs_sent(&self) -> usize {
        self.msgs_sent
    }

    fn msgs_recv(&self) -> usize {
        self.msgs_recv
    }
}

impl ByteStats for ConnectionInfo {
    fn bytes_sent(&self) -> usize {
        self.bytes_sent
    }

    fn bytes_recv(&self) -> usize {
        self.bytes_recv
    }
}

/// Maximum number of [`aeronet::LaneKey`]s which the protocol can support in
/// a WebTransport transport.
pub const MAX_NUM_LANES: u8 = u8::MAX;

/// Error that occurs while processing a WebTransport transport.
#[derive(Derivative, thiserror::Error)]
#[derivative(
    Debug(bound = "<<P as TransportProtocol>::Send as TryAsBytes>::Error: Debug, <<P as TransportProtocol>::Recv as TryFromBytes>::Error: Debug"),
    //Clone(bound = "<<P as TransportProtocol>::Send as TryAsBytes>::Error: Debug, <<P as TransportProtocol>::Recv as TryFromBytes>::Error: Debug")
)]
pub enum WebTransportError<P>
where
    P: LaneProtocol,
    P::Send: TryAsBytes,
    P::Recv: TryFromBytes,
{
    #[error("backend closed")]
    BackendClosed,
    #[error("not connected")]
    NotConnected,
    #[error("failed to create endpoint")]
    CreateEndpoint(#[source] io::Error),
    #[error("failed to get local socket address")]
    GetLocalAddr(#[source] io::Error),
    #[error("failed to connect")]
    Connect(#[source] ConnectingError),
    #[error("disconnected")]
    Disconnected(#[source] ConnectionError),
    #[error("sending on {lane:?}")]
    Send {
        lane: P::Lane,
        #[source]
        source: LaneError<P>,
    },
    #[error("receiving")]
    Recv(#[source] LaneError<P>),
}

#[derive(Derivative, thiserror::Error)]
#[derivative(Debug(
    bound = "<P::Send as TryAsBytes>::Error: Debug, <P::Recv as TryFromBytes>::Error: Debug"
))]
pub enum LaneError<P>
where
    P: TransportProtocol,
    P::Send: TryAsBytes,
    P::Recv: TryFromBytes,
{
    #[error("failed to open stream")]
    OpenStream(#[source] ConnectionError),
    #[error("failed to await opening stream")]
    OpeningStream(#[source] StreamOpeningError),
    #[error("failed to accept stream")]
    AcceptStream(#[source] ConnectionError),

    // send
    #[error("failed to serialize message")]
    Serialize(#[source] <P::Send as TryAsBytes>::Error),
    #[error("failed to create packet")]
    CreatePacket(#[source] PacketError),
    #[error("failed to send datagram")]
    SendDatagram(#[source] SendDatagramError),

    // recv
    #[error("failed to receive packet")]
    RecvPacket(#[source] PacketError),
    #[error("failed to deserialize message")]
    Deserialize(#[source] <P::Recv as TryFromBytes>::Error),
}
