use bytes::Bytes;

use crate::ClientId;

#[derive(Debug, thiserror::Error)]
pub enum ServerTransportError {
    #[error("no client with id {id}")]
    NoClient { id: ClientId },
    #[error("internal error")]
    Internal(
        #[from]
        #[source]
        anyhow::Error,
    ),
}

pub trait ServerTransport {
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

    fn recv(&mut self, client_id: ClientId) -> Option<Result<Bytes, ServerTransportError>>;

    fn send(
        &mut self,
        client_id: ClientId,
        msg: impl Into<Bytes>,
    ) -> Result<(), ServerTransportError>;
}
