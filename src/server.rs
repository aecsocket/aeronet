use crate::{ClientId, TransportSettings};

#[derive(Debug, thiserror::Error)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Event))]
pub enum ServerTransportError {
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

pub trait ServerTransport<S: TransportSettings> {
    /// This function deliberately does not return an iterator, because that would mean that while
    /// iterating, a shared reference to this transport would be kept. A typical usage pattern is
    /// ```ignore
    /// for client_id in transport.clients() {
    ///     transport.recv(client_id);
    /// }
    /// ```
    /// If this function returned an iterator, `recv` could not be used because that takes an
    /// exclusive reference. You would have to manually collect the iterator into a Vec before
    /// iterating over it.
    fn clients(&self) -> Vec<ClientId>;

    fn recv(&mut self, from: ClientId) -> Option<Result<S::C2S, ServerTransportError>>;

    fn send(&mut self, to: ClientId, msg: impl Into<S::S2C>) -> Result<(), ServerTransportError>;
}
