#[cfg(feature = "bevy")]
mod plugin;

#[cfg(feature = "bevy")]
pub use plugin::*;

use std::{
    error::Error,
    fmt::{self, Debug},
    time::Instant,
};

use derivative::Derivative;

use crate::TransportProtocol;

pub trait ClientTransport<P>
where
    P: TransportProtocol,
{
    type Error: Error + Send + Sync + 'static;

    type ClientInfo;

    fn client_state(&self) -> ClientState<Self::ClientInfo>;

    fn send(&self, msg: impl Into<P::Send>) -> Result<(), Self::Error>;

    /// If this emits an event which changes the transport's state, then after
    /// this call, the transport will be in this new state.
    fn update(&mut self) -> impl Iterator<Item = ClientEvent<P, Self::Error>>;
}

slotmap::new_key_type! {
    /// Unique key identifying a client connected to a server.
    ///
    /// This key is unique for each individual session that a server accepts,
    /// even if a new client takes the slot/allocation of a previous client. To
    /// enforce this behavior, the key is implemented as a
    /// [`slotmap::new_key_type`] and intended to be used in a
    /// [`slotmap::SlotMap`].
    pub struct ClientKey;
}

impl fmt::Display for ClientKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

#[derive(Debug, Clone)]
pub enum ClientState<I> {
    Disconnected,
    Connecting,
    Connected { info: I },
}

#[derive(Derivative)]
#[derivative(
    Debug(bound = "P::Recv: Debug, E: Debug"),
    Clone(bound = "P::Recv: Clone, E: Clone")
)]
pub enum ClientEvent<P, E>
where
    P: TransportProtocol,
    E: Error,
{
    // state
    Connecting,
    Connected,
    Disconnected { reason: E },

    // messages
    Recv { msg: P::Recv, at: Instant },
}
