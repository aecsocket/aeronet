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

    type OpeningInfo: Send + Sync + 'static;

    type OpenInfo: Send + Sync + 'static;

    type ConnectingInfo: Send + Sync + 'static;

    type ConnectedInfo: Send + Sync + 'static;

    fn state(&self) -> ServerState<Self::OpeningInfo, Self::OpenInfo>;

    fn client_state(
        &self,
        client: ClientKey,
    ) -> ClientState<Self::ConnectingInfo, Self::ConnectedInfo>;

    fn clients(&self) -> impl Iterator<Item = ClientKey> + '_;

    fn send(&mut self, client: ClientKey, msg: impl Into<P::S2C>) -> Result<(), Self::Error>;

    fn disconnect(&mut self, client: ClientKey) -> Result<(), Self::Error>;

    fn update(
        &mut self,
    ) -> impl Iterator<Item = ServerEvent<P, Self::ConnectingInfo, Self::ConnectedInfo, Self::Error>> + '_;
}

#[derive(Debug, Clone)]
pub enum ServerState<A, B> {
    Closed,
    Opening(A),
    Open(B),
}

impl<A, B> ServerState<A, B> {
    pub fn is_closed(&self) -> bool {
        match self {
            Self::Closed => true,
            _ => false,
        }
    }

    pub fn is_opening(&self) -> bool {
        match self {
            Self::Opening(_) => true,
            _ => false,
        }
    }

    pub fn is_open(&self) -> bool {
        match self {
            Self::Open(_) => true,
            _ => false,
        }
    }
}

#[derive(Derivative)]
#[derivative(
    Debug(bound = "P::C2S: Debug, A: Debug, B: Debug, E: Debug"),
    Clone(bound = "P::C2S: Clone, A: Clone, B: Clone, E: Clone")
)]
pub enum ServerEvent<P, A, B, E>
where
    P: TransportProtocol,
    E: Error,
{
    // client state
    Connecting {
        client: ClientKey,
        info: A,
    },
    Connected {
        client: ClientKey,
        info: B,
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
