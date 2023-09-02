use crate::TransportSettings;

#[derive(Debug, thiserror::Error)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Event))]
pub enum ClientTransportError {
    #[error("failed to receive server data")]
    Recv(#[source] anyhow::Error),
    #[error("failed to send data to server")]
    Send(#[source] anyhow::Error),
}

pub trait ClientTransport<S: TransportSettings> {
    fn recv(&mut self) -> Option<Result<S::S2C, ClientTransportError>>;

    fn send(&mut self, msg: impl Into<S::C2S>) -> Result<(), ClientTransportError>;
}
