use bytes::Bytes;

#[derive(Debug, thiserror::Error)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Event))]
pub enum ClientTransportError {
    #[error("internal error")]
    Internal(
        #[from]
        #[source]
        anyhow::Error,
    ),
}

pub trait ClientTransport {
    fn recv(&mut self) -> Option<Result<Bytes, ClientTransportError>>;

    fn send(&mut self, msg: impl Into<Bytes>) -> Result<(), ClientTransportError>;
}
