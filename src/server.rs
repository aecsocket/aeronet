use anyhow::Result;

use crate::{ClientId, TransportSettings};

#[derive(Debug, Clone)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Event))]
pub enum ServerTransportEvent {
    Connect { client: ClientId },
    Disconnect { client: ClientId },
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ServerClientsError {
    #[error("invalid client id `{0}`")]
    Invalid(ClientId),
    #[error("client with id `{0}` is already removed")]
    AlreadyRemoved(ClientId),
}

pub trait ServerTransport<S: TransportSettings> {
    fn recv_events(&mut self) -> Result<Option<ServerTransportEvent>>;

    fn recv(&mut self, from: ClientId) -> Result<Option<S::C2S>>;

    fn send(&mut self, to: ClientId, msg: impl Into<S::S2C>) -> Result<()>;

    fn disconnect(&mut self, client: ClientId) -> Result<()>;
}
