#![warn(clippy::all)]
//#![warn(missing_docs)]
#![doc = include_str!("../README.md")]

//mod client;
mod server;
mod shared;

use aeronet::{TryIntoBytes, TryFromBytes, Message, Rtt, RemoteAddr};
pub use wtransport;

pub use aeronet_wt_core::*;

//pub use client::*;
pub use server::{*, front::*};

use std::{io, net::SocketAddr, time::Duration};

use wtransport::{
    error::{ConnectionError, StreamOpeningError, StreamReadError, StreamWriteError, SendDatagramError},
    Connection,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointInfo {
    pub rtt: Duration,
    pub remote_addr: SocketAddr,
    pub max_datagram_size: Option<usize>,
}

impl EndpointInfo {
    pub fn from_connection(conn: &Connection) -> Self {
        Self {
            rtt: conn.rtt(),
            remote_addr: conn.remote_address(),
            max_datagram_size: conn.max_datagram_size(),
        }
    }
}

impl Rtt for EndpointInfo {
    fn rtt(&self) -> Duration {
        self.rtt
    }
}

impl RemoteAddr for EndpointInfo {
    fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WebTransportError<S, R>
where
    S: Message + TryIntoBytes,
    R: Message + TryFromBytes,
{
    #[error("backend closed")]
    BackendClosed,
    #[error("failed to create endpoint")]
    CreateEndpoint(#[source] io::Error),
    #[error("failed to receive incoming session")]
    IncomingSession(#[source] ConnectionError),
    #[error("failed to accept incoming session")]
    AcceptSession(#[source] ConnectionError),
    #[error("on {on:?}")]
    OnChannel {
        on: ChannelId,
        #[source]
        source: ChannelError<S, R>,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum ChannelError<S, R>
where
    S: Message + TryIntoBytes,
    R: Message + TryFromBytes,
{
    // opening
    #[error("failed to request to open stream")]
    RequestOpenStream(#[source] ConnectionError),
    #[error("failed to open stream")]
    OpenStream(#[source] StreamOpeningError),
    // send
    #[error("failed to serialize data")]
    Serialize(#[source] S::Error),
    #[error("failed to write stream")]
    WriteStream(#[source] StreamWriteError),
    #[error("failed to send datagram")]
    SendDatagram(#[source] SendDatagramError),
    // recv
    #[error("failed to read stream")]
    ReadStream(#[source] StreamReadError),
    #[error("failed to deserialize data")]
    Deserialize(#[source] R::Error),
}

impl<S, R> WebTransportError<S, R>
where
    S: Message + TryIntoBytes,
    R: Message + TryFromBytes,
{
    pub fn on(on: ChannelId, source: ChannelError<S, R>) -> Self {
        Self::OnChannel { on, source }
    }
}
