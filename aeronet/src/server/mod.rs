#[cfg(feature = "bevy")]
mod plugin;

#[cfg(feature = "bevy")]
pub use plugin::*;

use std::{fmt::Debug, time::Instant};

use derivative::Derivative;

use crate::{ClientKey, ClientState, MessageState, MessageTicket, TransportProtocol};

pub trait ServerTransport<P>
where
    P: TransportProtocol,
{
    type Error: Send + Sync + 'static;

    type ServerInfo;

    type ClientInfo;

    fn server_state(&self) -> ServerState<Self::ServerInfo>;

    fn client_state(&self, client: ClientKey) -> ClientState<Self::ClientInfo>;

    fn message_state(&self, client: ClientKey, msg: MessageTicket) -> MessageState;

    fn send(&self, client: ClientKey, msg: impl Into<P::S2C>)
        -> Result<MessageTicket, Self::Error>;

    fn update(&mut self) -> impl Iterator<Item = ServerEvent<P, Self>>
    where
        Self: Sized;

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
    Debug(bound = "P::C2S: Debug, T::Error: Debug"),
    Clone(bound = "P::C2S: Clone, T::Error: Clone")
)]
pub enum ServerEvent<P, T>
where
    P: TransportProtocol,
    T: ServerTransport<P>,
{
    // server state
    Opening,
    Opened,
    Closed {
        reason: T::Error,
    },

    // client state
    Connecting {
        client: ClientKey,
    },
    Connected {
        client: ClientKey,
    },
    Disconnected {
        client: ClientKey,
        reason: T::Error,
    },

    // messages
    Recv {
        client: ClientKey,
        msg: P::C2S,
        at: Instant,
    },
    Ack {
        client: ClientKey,
        msg: MessageTicket,
        at: Instant,
    },
    Nack {
        client: ClientKey,
        msg: MessageTicket,
        at: Instant,
    },
}
