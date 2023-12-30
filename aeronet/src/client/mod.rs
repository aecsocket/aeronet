#[cfg(feature = "bevy")]
mod plugin;

#[cfg(feature = "bevy")]
pub use plugin::*;

use std::{
    fmt::{self, Debug},
    time::Instant,
};

use derivative::Derivative;

use crate::{MessageState, MessageTicket, Transport, TransportProtocol};

pub trait ClientTransport<P>: Transport
where
    P: TransportProtocol,
{
    type Error: Send + Sync + 'static;

    type ClientInfo;

    fn client_state(&self) -> ClientState<Self::ClientInfo>;

    fn message_state(&self, msg: MessageTicket) -> MessageState;

    fn send(&self, msg: impl Into<P::Send>) -> Result<MessageTicket, Self::Error>;

    /// If this emits an event which changes the transport's state, then after
    /// this call, the transport will be in this new state.
    fn update(&mut self) -> impl Iterator<Item = ClientEvent<P, Self>>
    where
        Self: Sized;
}

slotmap::new_key_type! {
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
    Debug(bound = "P::Recv: Debug, T::Error: Debug"),
    Clone(bound = "P::Recv: Clone, T::Error: Clone")
)]
pub enum ClientEvent<P, T>
where
    P: TransportProtocol,
    T: ClientTransport<P>,
{
    // state
    Connecting,
    Connected,
    Disconnected { reason: T::Error },

    // messages
    Recv { msg: P::Recv, at: Instant },
    Ack { msg: MessageTicket, at: Instant },
    Nack { msg: MessageTicket, at: Instant },
}
