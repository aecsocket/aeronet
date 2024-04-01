mod backend;

pub use backend::open;

use std::{fmt::Display, net::SocketAddr};

use aeronet_proto::seq::Seq;
use steamworks::SteamError;

#[derive(Debug, Clone, Copy)]
pub enum ListenTarget {
    Ip(SocketAddr),
    Peer { virtual_port: i32 },
}

slotmap::new_key_type! {
    pub struct ClientKey;
}

impl Display for ClientKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ServerMessageKey {
    client_key: ClientKey,
    msg_seq: Seq,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectionResponse {
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum BackendError {
    #[error("invalid handle")]
    InvalidHandle,
    #[error("frontend closed")]
    FrontendClosed,
    #[error("internal channel disconnected")]
    ChannelDisconnected,

    #[error("failed to create listen socket")]
    CreateListenSocket,
    #[error("failed to accept client")]
    AcceptClient(#[source] SteamError),
    #[error("failed to send packet")]
    Send(#[source] SteamError),
}
