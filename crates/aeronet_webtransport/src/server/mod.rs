mod backend;
mod frontend;

use aeronet_io::connection::DisconnectReason;
use bytes::Bytes;
pub use frontend::*;

use bevy_ecs::prelude::*;
use bevy_reflect::prelude::*;
use futures::channel::{mpsc, oneshot};
use std::{collections::HashMap, net::SocketAddr, time::Duration};
use thiserror::Error;
use wtransport::error::ConnectionError;

use crate::session::{SessionError, SessionMeta};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Event, Reflect)]
pub enum ConnectionResponse {
    Accepted,
    Forbidden,
    NotFound,
}

/// [`WebTransportServer`] error.
#[derive(Debug, Error)]
pub enum ServerError {
    /// Failed to await an incoming session request.
    #[error("failed to await session request")]
    AwaitSessionRequest(#[source] ConnectionError),
    /// User rejected this incoming session request.
    #[error("user rejected session request")]
    Rejected,
    /// Failed to accept the incoming session request.
    #[error("failed to accept session")]
    AcceptSessionRequest(#[source] ConnectionError),
    /// Generic session error.
    #[error(transparent)]
    Session(#[from] SessionError),
}

#[derive(Debug)]
struct ToOpen {
    local_addr: SocketAddr,
    recv_connecting: mpsc::Receiver<ToConnecting>,
}

#[derive(Debug)]
struct ToConnecting {
    authority: String,
    path: String,
    origin: Option<String>,
    user_agent: Option<String>,
    headers: HashMap<String, String>,
    send_session_entity: oneshot::Sender<Entity>,
    send_conn_response: oneshot::Sender<ConnectionResponse>,
    recv_dc: oneshot::Receiver<DisconnectReason<ServerError>>,
    recv_next: oneshot::Receiver<ToConnected>,
}

#[derive(Debug)]
struct ToConnected {
    initial_remote_addr: SocketAddr,
    initial_rtt: Duration,
    initial_mtu: usize,
    recv_meta: mpsc::Receiver<SessionMeta>,
    recv_packet_b2f: mpsc::Receiver<Bytes>,
    send_packet_f2b: mpsc::UnboundedSender<Bytes>,
    send_user_dc: oneshot::Sender<String>,
}
