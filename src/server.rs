use crate::{ClientId, TransportSettings};

#[derive(Debug, thiserror::Error)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Event))]
pub enum ServerTransportError {
    #[error("failed to receive client connection")]
    RecvClient,
    #[error("no client with id `{id}`")]
    NoClient { id: ClientId },
    #[error("failed to receive data from client `{from}`")]
    Recv {
        from: ClientId,
        #[source]
        source: anyhow::Error,
    },
    #[error("failed to send data to client `{to}`")]
    Send {
        to: ClientId,
        #[source]
        source: anyhow::Error,
    },
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Event))]
pub enum ServerTransportEvent {
    Connect { id: ClientId },
    Disconnect { id: ClientId },
}

pub trait ServerTransport<S: TransportSettings> {
    fn recv_events(&mut self) -> Option<Result<ServerTransportEvent, ServerTransportError>>;

    fn recv(&mut self, from: ClientId) -> Option<Result<S::C2S, ServerTransportError>>;

    fn send(&mut self, to: ClientId, msg: impl Into<S::S2C>) -> Result<(), ServerTransportError>;
}
