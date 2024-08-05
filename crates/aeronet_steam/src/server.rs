use core::fmt;

use aeronet::server::{ServerState, ServerTransport};
use aeronet_proto::session::MessageKey;

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct SteamServer {
    state: State,
    /// See [`ServerTransport::default_disconnect_reason`].
    ///
    /// [`ServerTransport::default_disconnect_reason`]: aeronet::server::ServerTransport::default_disconnect_reason
    pub default_disconnect_reason: Option<String>,
}

#[derive(Debug)]
enum State {
    Closed,
    Opening,
    Open,
    Closing { reason: String },
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ServerError {}

slotmap::new_key_type! {
    /// Key uniquely identifying a client in a [`WebTransportServer`].
    ///
    /// If the same physical client disconnects and reconnects (i.e. the same
    /// process), this counts as a new client.
    pub struct ClientKey;
}

impl fmt::Display for ClientKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

#[derive(Debug)]
pub struct Opening {}

#[derive(Debug)]
pub struct Open {}

#[derive(Debug)]
pub struct Connecting {}

#[derive(Debug)]
pub struct Connected {}

impl SteamServer {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: State::Closed,
            default_disconnect_reason: None,
        }
    }
}

impl ServerTransport for SteamServer {
    type Error = ServerError;

    type Opening<'this> = &'this Opening;

    type Open<'this> = &'this Open;

    type Connecting<'this> = &'this Connecting;

    type Connected<'this> = &'this Connected;

    type ClientKey = ClientKey;

    type MessageKey = MessageKey;

    fn state(&self) -> ServerState<Self::Opening<'_>, Self::Open<'_>> {}
}
