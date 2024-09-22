//! Allows creating a dedicated server, which listens for client connections
//! and coordinates messaging between multiple clients.
//!
//! See [`WebTransportServer`].

mod backend;
mod frontend;

pub use frontend::*;
use {
    crate::session::{SessionError, SessionMeta},
    aeronet_io::connection::DisconnectReason,
    bevy_ecs::prelude::*,
    bevy_reflect::prelude::*,
    bytes::Bytes,
    futures::channel::{mpsc, oneshot},
    std::{collections::HashMap, net::SocketAddr, time::Duration},
    thiserror::Error,
    wtransport::error::ConnectionError,
};

/// Configuration for the [`WebTransportServer`].
pub type ServerConfig = wtransport::ServerConfig;

/// How should a [`WebTransportServer`] respond to a client wishing to connect
/// to the server?
///
/// After observing a [`Trigger<SessionRequest>`], trigger this event on the
/// client to determine if the client should be allowed to connect or not.
///
/// If you do not trigger [`SessionResponse`], then the client will never
/// connect.
///
/// # Examples
///
/// Accept all clients without any extra checks:
///
/// ```
/// use {
///     aeronet_webtransport::server::{SessionRequest, SessionResponse},
///     bevy_ecs::prelude::*,
/// };
///
/// fn on_session_request(trigger: Trigger<SessionRequest>, mut commands: Commands) {
///     let client = trigger.entity();
///     commands.trigger_targets(SessionResponse::Accepted, client);
/// }
/// ```
///
/// Check if the client has a given header before accepting them:
///
/// ```
/// use {
///     aeronet_webtransport::server::{SessionRequest, SessionResponse},
///     bevy_ecs::prelude::*,
/// };
///
/// fn on_session_request(trigger: Trigger<SessionRequest>, mut commands: Commands) {
///     let client = trigger.entity();
///     let request = trigger.event();
///
///     let mut response = SessionResponse::Forbidden;
///     if let Some(auth_token) = request.headers.get(":auth-token") {
///         if validate_auth_token(auth_token) {
///             response = SessionResponse::Accepted;
///         }
///     }
///
///     commands.trigger_targets(response, client);
/// }
/// # fn validate_auth_token(_: &str) -> bool { unimplemented!() }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Event, Reflect)]
pub enum SessionResponse {
    /// Allow the client to connect to the server.
    Accepted,
    /// Reject the client with a `403 Forbidden`.
    Forbidden,
    /// Reject the client with a `404 Not Found`.
    NotFound,
}

/// Triggered when a client requests to connect to a [`WebTransportServer`].
///
/// Use the fields in this event to decide whether to accept the client's
/// connection or not by triggering [`SessionResponse`] on this client.
///
/// If you do not trigger [`SessionResponse`], then the client will never
/// connect.
#[derive(Debug, Clone, PartialEq, Eq, Event, Reflect)]
pub struct SessionRequest {
    /// `:authority` header.
    pub authority: String,
    /// `:path` header.
    pub path: String,
    /// `:origin` header.
    pub origin: Option<String>,
    /// `:user-agent` header.
    pub user_agent: Option<String>,
    /// Full map of request headers.
    pub headers: HashMap<String, String>,
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
    send_session_response: oneshot::Sender<SessionResponse>,
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
