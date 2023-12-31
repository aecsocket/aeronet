#[cfg(feature = "bevy")]
mod plugin;

#[cfg(feature = "bevy")]
pub use plugin::*;

use std::{error::Error, fmt::Debug, time::Instant};

use derivative::Derivative;

use crate::{ClientKey, ClientState, TransportProtocol};

pub trait ServerTransport<P>
where
    P: TransportProtocol,
{
    type Error: Error + Send + Sync + 'static;

    type ServerInfo;

    type ClientInfo;

    fn server_state(&self) -> ServerState<Self::ServerInfo>;

    fn client_state(&self, client: ClientKey) -> ClientState<Self::ClientInfo>;

    fn send(&mut self, client: ClientKey, msg: impl Into<P::S2C>) -> Result<(), Self::Error>;

    fn update(&mut self) -> impl Iterator<Item = ServerEvent<P, Self::Error>>;

    fn disconnect(&mut self, client: ClientKey) -> Result<(), Self::Error>;
}

#[derive(Debug, Clone)]
pub enum ServerState<I> {
    Closed,
    Opening,
    Open { info: I },
}

#[derive(Derivative)]
#[derivative(
    Debug(bound = "P::C2S: Debug, E: Debug"),
    Clone(bound = "P::C2S: Clone, E: Clone")
)]
pub enum ServerEvent<P, E>
where
    P: TransportProtocol,
    E: Error,
{
    // client state
    Connecting {
        client: ClientKey,
    },
    Connected {
        client: ClientKey,
    },
    Disconnected {
        client: ClientKey,
        reason: E,
    },

    // messages
    Recv {
        client: ClientKey,
        msg: P::C2S,
        at: Instant,
    },
}
